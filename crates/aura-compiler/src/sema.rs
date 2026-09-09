mod capabilities;
use capabilities::resolve_param_passings;
pub(crate) use capabilities::{assertion_dispatch_is_non_consuming, resolve_param_passing};
mod callables;
use callables::{
    capturing_closure_branch_diagnostic, capturing_closure_branch_mismatch,
    closure_signature_matches_function, default_argument_references_param,
    erase_type_callable_contracts, function_type_mismatch_message, merge_type_callable_contracts,
    required_ordered_arg, ClosureArgumentPolicy, LambdaTypingRequest,
};
pub use callables::{
    ClosureCallKind, ClosureCapture, ClosureCaptureMode, ClosureId, ClosureInfo, ClosureOwner,
    FunctionParamContract,
};
mod resolve;
use resolve::{find_namespace_in_modules, reject_reserved_type_name, validate_type_params};
mod traits;
pub(crate) use traits::{
    binary_operator_trait, merge_trait_bounds, self_type_substitutions, unary_operator_trait,
};
use traits::{
    lower_supertraits, lower_trait_bounds, lower_trait_bounds_with_self,
    reject_builtin_trait_method_collisions,
};

mod places;
#[cfg(test)]
use places::PlaceProjection;
use places::{PlacePath, ProjectionPath};
mod loans;
#[cfg(test)]
use loans::block_end_span;
use loans::{
    last_name_reference_span_in_match, last_name_reference_span_in_stmt, view_return_contract_key,
    ActiveMatchBorrow, BorrowedCallPlace, ViewBinding,
};

#[cfg(test)]
use program::register_module_namespace_types;
#[cfg(test)]
use properties::{
    copy_class_info_from_modules, copy_enum_info_from_modules, type_is_copy_in_context,
};
#[cfg(test)]
use types::type_pattern_specificity;
mod program;
pub use program::{
    AliasInfo, ClassInfo, ConstantInfo, EnumInfo, EnumPayloadFieldInfo, EnumVariantInfo,
    ExternFunctionInfo, FieldInfo, FunctionInfo, FunctionSignature, ImportedBinding, MethodInfo,
    ModuleContext, ModuleNamespace, OpaqueHandleInfo, Program, TraitImplInfo, TraitImplMethodInfo,
    TraitInfo, TraitMethodInfo,
};
mod aliases;
pub(crate) use aliases::expand_alias_callee;
mod type_budget;
#[cfg(test)]
mod type_budget_tests;
mod types;
use types::{
    collect_type_params_from_type, collect_type_ref_type_params, has_unresolved_type_params,
    lower_type, lower_type_with_self, merged_type_param_scope, substitute_trait_bounds,
    type_param_scope, unify_type_pattern,
};
pub(crate) use types::{
    substitute_alias_type, substitute_trait_bound, substitute_type,
    substitutions_from_decl_type_args, trait_impl_specificity, trait_impl_specificity_parts,
    type_pattern_matches,
};
pub use types::{TraitBound, Type, TypeDefinitions};
mod properties;
pub(crate) use properties::integer_type_bounds;
use properties::{
    array_element_type, is_array_dtype, is_builtin_copy_named_type, is_builtin_io_resource_type,
    is_builtin_type, is_float_type, is_integer_type, is_numeric_type, is_option_type,
    is_string_type, map_key_value_types, preserves_qualified_builtin_type_name,
    rng_clone_obligation_params_in_context_with_modules, rng_clone_safety_in_context_with_modules,
    set_element_type, type_contains_closure_value, type_contains_loan_closure, type_contains_named,
    type_is_copy_in_context_with_modules, type_reaches_class_through_non_indirect_fields,
    vec_element_type, RngCloneSafety, TaskObservationSummary,
};

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, ClassDecl, CompareLink, ComprehensionClause,
    ComprehensionOutput, EnumDecl, Expr, ExprKind, FunctionDecl, ImplDecl, Item, LambdaParam,
    LiteralPattern, LiteralPatternKind, MatchExprArm, MatchStmt, Module, Param, ParamMode, Pattern,
    ReceiverKind, Stmt, TraitDecl, TypeRef, UnaryOp, VariantPattern, WithStmt,
};
use crate::call::{
    bind_call_arguments, callable_params_from_decl, BuiltinAssociatedFunction,
    BuiltinClassConstructor, BuiltinFunction, BuiltinMember, CallConvention,
};
use crate::diag::{Diagnostic, Result};
use crate::integer::{
    integer_type_bounds as integer_type_bounds_impl, IntegerBounds, IntegerValue,
};
use crate::runtime_value::{parse_format_spec, validate_format_spec_for_type};

const NO_IDENTITY_EQUALITY_NOTE: &str =
    "Aura has no identity-equality fallback; equality-dependent operations require a defined value relation";

pub(crate) fn is_duration_type(ty: &Type) -> bool {
    *ty == Type::named("Duration")
}

pub(crate) fn builtin_duration_binary_result(
    op: BinaryOp,
    left_ty: &Type,
    right_ty: &Type,
) -> Option<Type> {
    let duration = Type::named("Duration");
    let int64 = Type::named("int64");
    match op {
        BinaryOp::Add | BinaryOp::Sub if left_ty == &duration && right_ty == &duration => {
            Some(duration)
        }
        BinaryOp::Mul
            if (left_ty == &duration && right_ty == &int64)
                || (left_ty == &int64 && right_ty == &duration) =>
        {
            Some(duration)
        }
        BinaryOp::FloorDiv if left_ty == &duration && right_ty == &int64 => Some(duration),
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Less
        | BinaryOp::LessEq
        | BinaryOp::Greater
        | BinaryOp::GreaterEq
            if left_ty == &duration && right_ty == &duration =>
        {
            Some(Type::named("bool"))
        }
        _ => None,
    }
}

/// Stable semantic identity for one comprehension expression.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ComprehensionId {
    pub module_name: String,
    pub owner: ClosureOwner,
    pub line: usize,
    pub column: usize,
}

impl ComprehensionId {
    pub fn new(module_name: &str, owner: ClosureOwner, span: crate::diag::Span) -> Self {
        Self {
            module_name: module_name.to_string(),
            owner,
            line: span.line,
            column: span.column,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComprehensionClauseInfo {
    pub binding_type: Type,
    /// Queue iteration receives an owned payload. Other bare comprehension
    /// sources yield copy values or shared collection elements.
    pub receive_owned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComprehensionInfo {
    pub id: ComprehensionId,
    pub result_type: Type,
    pub clauses: Vec<ComprehensionClauseInfo>,
}

pub(crate) fn type_is_copy_in_program(ty: &Type, program: &Program) -> bool {
    type_is_copy_in_context_with_modules(
        ty,
        &program.classes,
        &program.enums,
        &program.imported_modules,
        &program.module_registry,
    )
}

#[cfg(test)]
fn check(module: Module) -> Result<Program> {
    check_with_context(module, ModuleContext::default())
}

pub(crate) fn check_with_context(module: Module, context: ModuleContext) -> Result<Program> {
    program::check_with_context(module, context)
}

fn validate_params(receiver: Option<ReceiverKind>, params: &[Param], owner: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for param in params {
        if receiver.is_some() && param.name == "self" {
            return Err(Diagnostic::at(
                param.span,
                format!("parameter `self` conflicts with the receiver on {}", owner),
            ));
        }
        if !seen.insert(&param.name) {
            return Err(Diagnostic::at(
                param.span,
                format!("duplicate parameter `{}` on {}", param.name, owner),
            ));
        }
    }
    Ok(())
}

fn validate_ffi_signature(
    decl: &crate::ast::ExternFunctionDecl,
    opaque_handles: &BTreeMap<String, OpaqueHandleInfo>,
    type_names: &TypeDefinitions,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
) -> Result<()> {
    // The parser accepts only the ratified `extern "C"` spelling and owns its
    // user-facing diagnostic. Semantic checking therefore never receives an
    // alternate ABI from source and need not duplicate that unreachable gate.
    validate_params(
        None,
        &decl.params,
        &format!("extern function `{}`", decl.name),
    )?;

    for param in &decl.params {
        if matches!(param.ty.kind, crate::ast::TypeRefKind::Function { .. }) {
            return Err(Diagnostic::coded_at(
                "AU2005",
                param.ty.span,
                format!(
                    "FFI callbacks are reserved; parameter `{}` cannot use a function type",
                    param.name
                ),
            )
            .with_help(
                "write a named Aura wrapper around a fixed extern declaration; C-to-Aura callbacks are not supported in FFI v0",
            ));
        }
        if ffi_type_ref_is_raw_pointer(&param.ty) {
            return Err(Diagnostic::coded_at(
                "AU2005",
                param.ty.span,
                format!(
                    "FFI raw pointers are reserved; parameter `{}` must use a str, list[uint8], or opaque handle contract",
                    param.name
                ),
            )
            .with_help(
                "use bare `str` or `list[uint8]` for a const pointer-length view, `mut list[uint8]` for fixed-length copy-in/out, or declare an opaque handle",
            ));
        }
        let ty = lower_type(
            &param.ty,
            type_names,
            type_arities,
            canonical_type_names,
            &BTreeMap::new(),
        )?;
        validate_ffi_parameter(&param.name, param.mode, &ty, param.span, opaque_handles)?;
    }

    if matches!(
        decl.return_type.kind,
        crate::ast::TypeRefKind::Function { .. }
    ) {
        return Err(Diagnostic::coded_at(
            "AU2005",
            decl.return_type.span,
            "FFI callbacks are reserved; an extern declaration cannot return a function value",
        ));
    }
    if ffi_type_ref_is_raw_pointer(&decl.return_type) {
        return Err(Diagnostic::coded_at(
            "AU2005",
            decl.return_type.span,
            "FFI raw-pointer returns are reserved; use an opaque handle return type",
        ));
    }
    let return_type = lower_type(
        &decl.return_type,
        type_names,
        type_arities,
        canonical_type_names,
        &BTreeMap::new(),
    )?;
    validate_ffi_return(&return_type, decl.return_type.span, opaque_handles)
}

fn ffi_type_ref_is_raw_pointer(ty: &TypeRef) -> bool {
    matches!(
        &ty.kind,
        crate::ast::TypeRefKind::Named { name, .. } if name == "Ptr"
    )
}

fn ffi_scalar_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Named(name, args)
            if args.is_empty()
                && matches!(
                    name.as_str(),
                    "bool"
                        | "int8"
                        | "int16"
                        | "int32"
                        | "int64"
                        | "uint8"
                        | "uint16"
                        | "uint32"
                        | "uint64"
                        | "float32"
                        | "float64"
                )
    )
}

fn ffi_opaque_handle(ty: &Type, handles: &BTreeMap<String, OpaqueHandleInfo>) -> bool {
    let Type::Named(name, args) = ty else {
        return false;
    };
    args.is_empty()
        && handles.iter().any(|(visible_name, handle)| {
            name == visible_name || name == &format!("{}.{}", handle.module_name, handle.decl.name)
        })
}

fn validate_ffi_parameter(
    param_name: &str,
    mode: ParamMode,
    ty: &Type,
    span: crate::diag::Span,
    opaque_handles: &BTreeMap<String, OpaqueHandleInfo>,
) -> Result<()> {
    if ffi_scalar_type(ty) {
        if mode != ParamMode::Default {
            return Err(Diagnostic::coded_at(
                "AU3004",
                span,
                format!(
                    "fixed-width scalar parameter `{param_name}` must use the bare capability"
                ),
            )
            .with_help(
                "remove `own` or `mut`; FFI v0 passes scalar bits by value and reserves pointer-style scalar parameters",
            ));
        }
        return Ok(());
    }

    if *ty == Type::named("str") {
        return match mode {
            ParamMode::Default => Ok(()),
            ParamMode::Own => Err(Diagnostic::coded_at(
                "AU3004",
                span,
                format!("str view parameter `{param_name}` must use the bare capability"),
            )
            .with_help(
                "use bare `str`; it passes a temporary const UTF-8 pointer and byte length for the duration of the call",
            )),
            ParamMode::BorrowMut => Err(Diagnostic::coded_at(
                "AU3004",
                span,
                format!("mutable str views are reserved for parameter `{param_name}`"),
            )
            .with_help(
                "use `mut list[uint8]` for fixed-length writable bytes, or bare `str` for a const UTF-8 view",
            )),
        };
    }

    if matches!(ty, Type::Named(name, args) if name == "list" && args == &[Type::named("uint8")]) {
        return match mode {
            ParamMode::Default | ParamMode::BorrowMut => Ok(()),
            ParamMode::Own => Err(Diagnostic::coded_at(
                "AU3004",
                span,
                format!("owned byte views are reserved for parameter `{param_name}`"),
            )
            .with_help(
                "use bare `list[uint8]` for a const view or `mut list[uint8]` for fixed-length copy-in/out",
            )),
        };
    }

    if matches!(ty, Type::Named(name, args) if name == "list" && args.len() == 1) {
        return Err(Diagnostic::coded_at(
            "AU2002",
            span,
            format!(
                "only `list[uint8]` is supported as an FFI byte view; parameter `{param_name}` has `{ty}`"
            ),
        ));
    }

    if ffi_opaque_handle(ty, opaque_handles) {
        return match mode {
            ParamMode::Default | ParamMode::Own => Ok(()),
            ParamMode::BorrowMut => Err(Diagnostic::coded_at(
                "AU3004",
                span,
                format!("mutable opaque-handle parameters are reserved for `{param_name}`"),
            )
            .with_help(
                "use a bare handle to share it for the call, or `own Handle` when the C function consumes it",
            )),
        };
    }

    Err(Diagnostic::coded_at(
        "AU2002",
        span,
        format!("FFI v0 does not support parameter type `{ty}`"),
    )
    .with_help(
        "use a fixed-width scalar, bare str, bare or mut list[uint8], or a declared opaque handle",
    ))
}

fn validate_ffi_return(
    ty: &Type,
    span: crate::diag::Span,
    opaque_handles: &BTreeMap<String, OpaqueHandleInfo>,
) -> Result<()> {
    if *ty == Type::Unit || ffi_scalar_type(ty) || ffi_opaque_handle(ty, opaque_handles) {
        return Ok(());
    }
    if *ty == Type::named("str") {
        return Err(Diagnostic::coded_at(
            "AU2002",
            span,
            "FFI v0 cannot return a str view because no foreign lifetime or allocator contract exists",
        ));
    }
    if matches!(ty, Type::Named(name, args) if name == "list" && args == &[Type::named("uint8")]) {
        return Err(Diagnostic::coded_at(
            "AU2002",
            span,
            "FFI v0 cannot return a list[uint8] view because no foreign lifetime or allocator contract exists",
        ));
    }
    Err(Diagnostic::coded_at(
        "AU2002",
        span,
        format!("FFI v0 does not support return type `{ty}`"),
    )
    .with_help("return None, a fixed-width scalar, or a declared opaque handle"))
}

pub(crate) fn expr_references_name(expr: &Expr, name: &str) -> bool {
    default_argument_references_param(expr, &[name.to_string()]).is_some()
}

fn grouped_expr(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Group(inner) => grouped_expr(inner),
        _ => expr,
    }
}

fn grouped_specialized_expr(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Group(inner) | ExprKind::Specialize { expr: inner, .. } => {
            grouped_specialized_expr(inner)
        }
        _ => expr,
    }
}

fn grouped_name(expr: &Expr) -> Option<&str> {
    match &grouped_expr(expr).kind {
        ExprKind::Name(name) => Some(name),
        _ => None,
    }
}

fn block_references_name(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|stmt| stmt_references_name(stmt, name))
}

pub(crate) fn stmt_references_name(stmt: &Stmt, name: &str) -> bool {
    let target_references = |target: &crate::ast::AssignTarget| match target {
        crate::ast::AssignTarget::Name(target) => target == name,
        crate::ast::AssignTarget::Member { object, .. } => expr_references_name(object, name),
        crate::ast::AssignTarget::Index { object, index } => {
            expr_references_name(object, name) || expr_references_name(index, name)
        }
    };
    match stmt {
        Stmt::Assign(assign) => {
            target_references(&assign.target) || expr_references_name(&assign.value, name)
        }
        Stmt::View(view) => expr_references_name(&view.source, name),
        Stmt::Destructure(destructure) => expr_references_name(&destructure.value, name),
        Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::Assert(assertion) => {
            expr_references_name(&assertion.condition, name)
                || assertion
                    .message
                    .as_ref()
                    .is_some_and(|message| expr_references_name(message, name))
        }
        Stmt::Return(return_stmt) => return_stmt
            .value
            .as_ref()
            .is_some_and(|value| expr_references_name(value, name)),
        Stmt::If(if_stmt) => {
            if_stmt.branches.iter().any(|branch| {
                expr_references_name(&branch.condition, name)
                    || block_references_name(&branch.body, name)
            }) || if_stmt
                .else_body
                .as_ref()
                .is_some_and(|body| block_references_name(body, name))
        }
        Stmt::Match(match_stmt) => {
            expr_references_name(&match_stmt.scrutinee, name)
                || match_stmt.arms.iter().any(|arm| {
                    arm.guard
                        .as_ref()
                        .is_some_and(|guard| expr_references_name(guard, name))
                        || block_references_name(&arm.body, name)
                })
        }
        Stmt::For(for_stmt) => {
            expr_references_name(&for_stmt.iterable, name)
                || block_references_name(&for_stmt.body, name)
        }
        Stmt::With(with_stmt) => {
            expr_references_name(&with_stmt.value, name)
                || block_references_name(&with_stmt.body, name)
        }
        Stmt::While(while_stmt) => {
            expr_references_name(&while_stmt.condition, name)
                || block_references_name(&while_stmt.body, name)
        }
        Stmt::Expr(expr_stmt) => expr_references_name(&expr_stmt.expr, name),
    }
}

fn statement_span(stmt: &Stmt) -> crate::diag::Span {
    match stmt {
        Stmt::Assign(stmt) => stmt.span,
        Stmt::View(stmt) => stmt.span,
        Stmt::Destructure(stmt) => stmt.span,
        Stmt::Pass(stmt) => stmt.span,
        Stmt::Assert(stmt) => stmt.span,
        Stmt::Return(stmt) => stmt.span,
        Stmt::If(stmt) => stmt.span,
        Stmt::Match(stmt) => stmt.span,
        Stmt::For(stmt) => stmt.span,
        Stmt::With(stmt) => stmt.span,
        Stmt::While(stmt) => stmt.span,
        Stmt::Break(stmt) => stmt.span,
        Stmt::Continue(stmt) => stmt.span,
        Stmt::Expr(stmt) => stmt.span,
    }
}

fn collect_binding_target_names(target: &crate::ast::BindingTarget, names: &mut BTreeSet<String>) {
    match target {
        crate::ast::BindingTarget::Name { name, .. } => {
            names.insert(name.clone());
        }
        crate::ast::BindingTarget::Tuple { elements, .. } => {
            for element in elements {
                collect_binding_target_names(element, names);
            }
        }
    }
}

fn recursive_field_message(class_name: &str, field_name: &str, field_type: &TypeRef) -> String {
    if matches!(&field_type.kind, crate::ast::TypeRefKind::Tuple(_)) {
        return format!(
            "recursive field `{field_name}` on class `{class_name}` contains tuple storage; tuple types cannot be `indirect`, so move the recursive link into an `indirect` named field"
        );
    }
    format!("recursive field `{field_name}` on class `{class_name}` requires `indirect`")
}

/// The element, key, or substring type an `in` container compares against.
pub(crate) fn membership_needle_type(container_ty: &Type) -> Option<Type> {
    match container_ty {
        Type::Named(name, args) if (name == "list" || name == "set") && args.len() == 1 => {
            Some(args[0].clone())
        }
        Type::Named(name, args) if name == "dict" && args.len() == 2 => Some(args[0].clone()),
        Type::Named(name, args) if name == "str" && args.is_empty() => Some(Type::named("str")),
        _ => None,
    }
}

/// The builtin member that `in` delegates to for a supported container.
pub(crate) fn membership_member_name(container_ty: &Type) -> Option<&'static str> {
    match container_ty {
        Type::Named(name, args) if name == "dict" && args.len() == 2 => Some("contains_key"),
        Type::Named(name, args) if (name == "list" || name == "set") && args.len() == 1 => {
            Some("contains")
        }
        Type::Named(name, args) if name == "str" && args.is_empty() => Some("contains"),
        _ => None,
    }
}

fn unsupported_array_operator_name(op: BinaryOp) -> &'static str {
    const NAMES: &[(BinaryOp, &str)] = &[
        (BinaryOp::FloorDiv, "//"),
        (BinaryOp::Mod, "%"),
        (BinaryOp::Less, "<"),
        (BinaryOp::LessEq, "<="),
        (BinaryOp::Greater, ">"),
        (BinaryOp::GreaterEq, ">="),
    ];
    NAMES
        .iter()
        .find_map(|(candidate, name)| (*candidate == op).then_some(*name))
        .unwrap_or("logical operator")
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LiteralPatternKey {
    Int(IntegerValue),
    Float(u64),
    Bool(bool),
    String(String),
}

fn render_literal_pattern_key(key: &LiteralPatternKey) -> String {
    match key {
        LiteralPatternKey::Int(value) => value.to_string(),
        LiteralPatternKey::Float(bits) => f64::from_bits(*bits).to_string(),
        LiteralPatternKey::Bool(value) => value.to_string(),
        LiteralPatternKey::String(value) => format!("{:?}", value),
    }
}

fn pattern_contains_variant_shape(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Or(pattern) => pattern
            .alternatives
            .iter()
            .any(pattern_contains_variant_shape),
        Pattern::Tuple(pattern) => pattern.elements.iter().any(pattern_contains_variant_shape),
        Pattern::Variant(_) => true,
        Pattern::Binding(_) | Pattern::Literal(_) | Pattern::Wildcard(_) => false,
    }
}

#[derive(Clone)]
struct LocalBinding {
    ty: Type,
    assignable: bool,
    mutable_place: bool,
    managed_resource: bool,
    passing: ReceiverKind,
    borrow_origin: Option<String>,
    borrowed_at: Option<crate::diag::Span>,
    /// The source place selected by a shared or mutable match. Pattern
    /// bindings and their scoped shared aliases retain this provenance so a
    /// later overlapping source change makes a subsequent use stale.
    match_borrow_place: Option<PlacePath>,
    stale_match_borrow_place: Option<PlacePath>,
    /// Set on a payload bound by a bare (shared) `match` over a named place.
    /// ADR-0022 Q2 requires moving such a payload out to name `match own` as
    /// the replacement instead of the generic borrowed-move wording.
    shared_match_scrutinee: Option<String>,
    moved: bool,
    moved_at: Option<crate::diag::Span>,
    moved_fields: BTreeMap<ProjectionPath, crate::diag::Span>,
    frozen_places: BTreeMap<PlacePath, crate::diag::Span>,
    /// Whole-arm logical shared access for a bare match on a copy scrutinee.
    /// Copy payload bindings are values, but ADR-0022 still keeps the selected
    /// scrutinee shared until its arm completes.
    shared_match_places: BTreeMap<PlacePath, crate::diag::Span>,
    /// True for a value stored in a lambda environment. Phase 6.3 has no
    /// mutable-call closure category, so mutable access to these places is
    /// rejected even when the outer binding was declared `mut`.
    captured: bool,
    /// Present only for an explicit ADR-0038 view binding. Match and loop
    /// borrows continue to use the older provenance fields without becoming
    /// first-class views.
    view: Option<ViewBinding>,
    closure_loans: Vec<ViewBinding>,
}

#[derive(Clone)]
struct ExprResultEntry {
    locals: HashMap<String, LocalBinding>,
    expected: Option<Type>,
}

/// A compiler-known `for` iterable form.
#[derive(Clone, Copy, Eq, PartialEq)]
enum LoopFormKind {
    Enumerate,
    Zip,
}

impl LoopFormKind {
    fn name(self) -> &'static str {
        match self {
            Self::Enumerate => "enumerate",
            Self::Zip => "zip",
        }
    }

    fn arity(self) -> usize {
        match self {
            Self::Enumerate => 1,
            Self::Zip => 2,
        }
    }
}

struct LoopForm<'a> {
    kind: LoopFormKind,
    name: &'static str,
    #[allow(dead_code)]
    span: crate::diag::Span,
    iterables: Vec<&'a Expr>,
}

/// The element type an index-addressable collection yields in a lockstep loop.
pub(crate) fn lockstep_element_type(iterable_ty: &Type) -> Option<Type> {
    match iterable_ty {
        Type::Named(name, args) if (name == "list" || name == "set") && args.len() == 1 => {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// One named field read out of a surrounding expression's result.
#[derive(Clone, Copy)]
struct ProjectedField<'a> {
    name: &'a str,
    span: crate::diag::Span,
}

/// The parts of an expression-form `match` that its typing needs.
#[derive(Clone, Copy)]
struct MatchExprParts<'a> {
    scrutinee: &'a Expr,
    borrow_mode: ReceiverKind,
    arms: &'a [MatchExprArm],
    span: crate::diag::Span,
}

/// How the surrounding expression uses a branching expression's result.
#[derive(Clone, Copy)]
enum BranchResultUse<'a> {
    /// The result is produced without transferring ownership at this point.
    Inspected,
    /// The result itself is consumed by the surrounding expression.
    Consumed,
    /// One field of the result is consumed by the surrounding expression.
    ProjectedField(ProjectedField<'a>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedUnaryOperatorAccess {
    return_type: Type,
    receiver_passing: ReceiverKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedBinaryOperatorAccess {
    return_type: Type,
    receiver_passing: ReceiverKind,
    rhs_passing: ReceiverKind,
}

struct FunctionChecker<'a> {
    root_module_name: &'a str,
    module_name: &'a str,
    type_names: &'a TypeDefinitions,
    type_arities: &'a BTreeMap<String, usize>,
    canonical_type_names: &'a BTreeMap<String, String>,
    classes: &'a BTreeMap<String, ClassInfo>,
    enums: &'a BTreeMap<String, EnumInfo>,
    functions: &'a BTreeMap<String, FunctionInfo>,
    constants: &'a BTreeMap<String, ConstantInfo>,
    extern_functions: &'a BTreeMap<String, ExternFunctionInfo>,
    opaque_handles: &'a BTreeMap<String, OpaqueHandleInfo>,
    traits: &'a BTreeMap<String, TraitInfo>,
    trait_impls: &'a [TraitImplInfo],
    imported_modules: &'a BTreeMap<String, ModuleNamespace>,
    module_registry: &'a BTreeMap<String, ModuleNamespace>,
    current_return_type: Option<Type>,
    type_params: BTreeMap<String, ()>,
    type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    implicit_borrowed_params: BTreeMap<String, Type>,
    active_match_borrow_places: Rc<RefCell<Vec<ActiveMatchBorrow>>>,
    rng_clone_obligations: Rc<RefCell<BTreeSet<String>>>,
    array_equality_obligations: Rc<RefCell<BTreeSet<String>>>,
    expr_result_entries: Rc<RefCell<HashMap<usize, ExprResultEntry>>>,
    closure_owner: ClosureOwner,
    closure_infos: Rc<RefCell<BTreeMap<ClosureId, ClosureInfo>>>,
    comprehension_infos: Rc<RefCell<BTreeMap<ComprehensionId, ComprehensionInfo>>>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlockFlow {
    FallsThrough,
    AlwaysReturns,
}

impl<'a> FunctionChecker<'a> {
    fn check_index_domain_type(
        &self,
        value: &Expr,
        span: crate::diag::Span,
        subject: &str,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let expected = Type::named("int64");
        let actual = if Self::is_integer_literal_expr(value) {
            self.type_of_expr_hint(value, locals, Some(&expected))?
        } else {
            self.type_of_expr(value, locals)?
        };
        // Pointer-sized integer domains vary by compilation target. Keeping
        // this exception target-stable requires an explicitly fixed-width
        // source type, so intsize/uintsize never receive implicit widening.
        let pointer_sized = matches!(
            &actual,
            Type::Named(name, args)
                if args.is_empty() && matches!(name.as_str(), "intsize" | "uintsize")
        );
        let losslessly_widens = !pointer_sized
            && match integer_type_bounds(&actual) {
                Some(IntegerBounds::Signed { min, max }) => {
                    min >= i64::MIN as i128 && max <= i64::MAX as i128
                }
                Some(IntegerBounds::Unsigned { max }) => max <= i64::MAX as u128,
                None => false,
            };
        if !losslessly_widens {
            return Err(Diagnostic::coded_at(
                "AU2002",
                span,
                format!("{subject} must have type `int64` or a losslessly narrower integer type, found `{actual}`"),
            ));
        }
        Ok(expected)
    }

    fn check_vec_index_type(
        &self,
        index: &Expr,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.check_index_domain_type(index, span, "list indices", locals)?;
        Ok(())
    }

    fn check_array_index_type(
        &self,
        index: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let coordinates = match &index.kind {
            ExprKind::Tuple(elements) => elements.as_slice(),
            _ => std::slice::from_ref(index),
        };
        for coordinate in coordinates {
            self.check_index_domain_type(coordinate, coordinate.span, "Array indices", locals)?;
        }
        Ok(())
    }

    fn check_slice_endpoint_type(
        &self,
        endpoint: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        self.check_index_domain_type(endpoint, endpoint.span, "slice endpoints", locals)
    }

    fn require_vec_orderable(
        &self,
        method_name: &str,
        subject: &str,
        ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        if self
            .type_of_binary(span, BinaryOp::Less, ty.clone(), ty.clone())
            .is_ok()
        {
            return Ok(());
        }
        Err(Diagnostic::coded_at(
            "AU2002",
            span,
            format!("`list.{method_name}` cannot order {subject} `{ty}`"),
        )
        .with_help(
            "use an existing naturally ordered type, or implement `Ord[T].lt` returning `bool`",
        ))
    }

    /// Builds the `AU3005` message for a rejected non-copy indexed read.
    ///
    /// The recommended recovery depends on whether the selected value can be
    /// cloned at all. Recommending `get(...)` unconditionally sends a caller
    /// holding non-cloneable `random.Rng` state to an `AU3007` dead end, so the
    /// guidance follows the same tri-state classification that rejection uses.
    fn indexed_read_guidance(&self, container: &str, selector: &str, ty: &Type) -> String {
        let transfer = if container == "dict" {
            format!("remove({selector})")
        } else {
            format!("pop({selector})")
        };
        if let Some(result_ty) = self.nonrepeatable_task_result_in(ty) {
            return format!(
                "cannot implicitly copy `{ty}` out of a {container} index; `get({selector})` cannot clone it because that would duplicate the single observation right for task result `{result_ty}`, so use `{transfer}` to transfer ownership instead"
            );
        }
        match self.rng_clone_safety(ty) {
            RngCloneSafety::Safe if container == "dict" => format!(
                "cannot implicitly copy `{ty}` out of a dict index; use `get(key)` for an explicit cloned optional read, or `remove(key)` to transfer ownership"
            ),
            RngCloneSafety::Safe => format!(
                "cannot implicitly copy `{ty}` out of a list index; use `get(index)` for an explicit cloned read instead"
            ),
            RngCloneSafety::ContainsRng => {
                let reason = Self::non_cloneable_rng_reason(ty);
                format!(
                    "cannot implicitly copy `{ty}` out of a {container} index; `get({selector})` cannot clone it because {reason}, so use `{transfer}` to transfer ownership instead"
                )
            }
            RngCloneSafety::Unknown => format!(
                "cannot implicitly copy `{ty}` out of a {container} index; `get({selector})` requires a clone-safe `{ty}`, or use `{transfer}` to transfer ownership"
            ),
        }
    }

    /// Builds the `AU3006` message for a rejected non-copy indexed compound
    /// assignment without recommending a clone path that the selected type
    /// cannot use.
    fn indexed_compound_assignment_guidance(
        &self,
        container: &str,
        selector: &str,
        ty: &Type,
    ) -> String {
        let transfer = if container == "dict" {
            format!("remove({selector})")
        } else {
            format!("pop({selector})")
        };
        let writeback = if container == "dict" {
            "indexed assignment"
        } else {
            "insert(index, value)"
        };
        if let Some(result_ty) = self.nonrepeatable_task_result_in(ty) {
            return format!(
                "cannot implicitly copy `{ty}` out of a {container} index for compound assignment; `get({selector})` cannot clone it because that would duplicate the single observation right for task result `{result_ty}`, so use `{transfer}` to transfer ownership; update the selected value, then write it back with `{writeback}`"
            );
        }
        match self.rng_clone_safety(ty) {
            RngCloneSafety::Safe if container == "dict" => format!(
                "cannot implicitly copy `{ty}` out of a dict index for compound assignment; use `get(key)` for an explicit cloned optional read, or `remove(key)` to transfer ownership; update the selected value, then write it back with indexed assignment"
            ),
            RngCloneSafety::Safe => format!(
                "cannot implicitly copy `{ty}` out of a list index for compound assignment; use `get(index)` for an explicit cloned optional read, update it, then write the result back with `set(index, value)`"
            ),
            RngCloneSafety::ContainsRng => {
                let reason = Self::non_cloneable_rng_reason(ty);
                format!(
                    "cannot implicitly copy `{ty}` out of a {container} index for compound assignment; `get({selector})` cannot clone it because {reason}, so use `{transfer}` to transfer ownership; update the selected value, then write it back with `{writeback}`"
                )
            }
            RngCloneSafety::Unknown => format!(
                "cannot implicitly copy `{ty}` out of a {container} index for compound assignment; `get({selector})` requires a clone-safe `{ty}`, or use `{transfer}` to transfer ownership; update the selected value, then write it back with `{writeback}`"
            ),
        }
    }

    fn nonrepeatable_task_result_in(&self, ty: &Type) -> Option<Type> {
        let mut nominals = BTreeMap::new();
        self.collect_transfer_nominals(ty, &mut nominals);
        let mut summaries = nominals
            .keys()
            .map(|key| (key.clone(), TaskObservationSummary::default()))
            .collect::<BTreeMap<_, _>>();
        loop {
            let mut changed = false;
            for (key, nominal) in &nominals {
                let derived = self.task_observation_nominal_summary(nominal, &summaries);
                let current = summaries
                    .get_mut(key)
                    .expect("every discovered task-observation nominal has a summary");
                changed |= Self::merge_task_observation_summary(current, derived);
            }
            if !changed {
                break;
            }
        }
        self.task_observation_shape(ty, &BTreeMap::new(), &summaries)
            .unconditional_result
    }

    fn opaque_handle_in_type(&self, ty: &Type) -> Option<Type> {
        self.opaque_handle_in_type_inner(ty, &mut BTreeSet::new())
    }

    fn require_array_equality_eligible(
        &self,
        ty: &Type,
        operation: impl Into<String>,
        span: crate::diag::Span,
    ) -> Result<()> {
        let operation = operation.into();
        if let Some(callable_ty) = self.callable_in_equality_type(ty) {
            return Err(Diagnostic::coded_at(
                "AU2008",
                span,
                format!("{operation} because `{callable_ty}` does not define equality"),
            )
            .with_note(NO_IDENTITY_EQUALITY_NOTE)
            .with_help(
                "compare explicit results or a stable discriminant; callable identity is not value equality",
            ));
        }
        if self.rng_clone_safety(ty) == RngCloneSafety::ContainsRng {
            return Err(Diagnostic::coded_at(
                "AU2008",
                span,
                format!("{operation} because `random.Rng` does not define equality"),
            )
            .with_note(NO_IDENTITY_EQUALITY_NOTE)
            .with_help(
                "compare generated scalar values or an explicit stable discriminant; generator identity is not value equality",
            ));
        }
        if let Some(handle_ty) = self.opaque_handle_in_type(ty) {
            return Err(Diagnostic::coded_at(
                "AU2008",
                span,
                format!(
                    "{operation} because opaque FFI handle `{handle_ty}` does not define equality"
                ),
            )
            .with_note(NO_IDENTITY_EQUALITY_NOTE)
            .with_help(
                "compare a stable scalar or str identifier exposed by the binding instead of foreign identity",
            ));
        }
        let Some(array_ty) = self.array_in_equality_type(ty) else {
            let obligations = self.array_equality_type_params(ty);
            self.array_equality_obligations
                .borrow_mut()
                .extend(obligations);
            return Ok(());
        };
        Err(Diagnostic::coded_at(
            "AU2003",
            span,
            format!(
                "{} because it contains `{array_ty}`, whose equality is unavailable",
                operation
            ),
        )
        .with_help(
            "compare Array elements explicitly, or compare a chosen scalar summary such as shape, length, or a reduction result",
        ))
    }

    fn opaque_handle_in_type_inner(
        &self,
        ty: &Type,
        visiting: &mut BTreeSet<String>,
    ) -> Option<Type> {
        if self.is_opaque_handle_type(ty) {
            return Some(ty.clone());
        }
        match ty {
            Type::Union(union) => union
                .members
                .iter()
                .find_map(|member| self.opaque_handle_in_type_inner(member, visiting)),
            Type::Tuple(elements) => elements
                .iter()
                .find_map(|element| self.opaque_handle_in_type_inner(element, visiting)),
            Type::Named(name, args) => {
                if let Some(handle) = args
                    .iter()
                    .find_map(|arg| self.opaque_handle_in_type_inner(arg, visiting))
                {
                    return Some(handle);
                }

                if let Some(class_info) = self.resolve_class_info(name) {
                    if args.len() != class_info.decl.type_params.len() {
                        return None;
                    }
                    let key = format!("class:{}:{}", class_info.module_name, class_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&class_info.decl.type_params, args);
                    let handle = class_info.fields.values().find_map(|field| {
                        let field_ty = substitute_type(&field.ty, &substitutions);
                        self.opaque_handle_in_type_inner(&field_ty, visiting)
                    });
                    visiting.remove(&key);
                    return handle;
                }

                if let Some(enum_info) = self.resolve_enum_info(name) {
                    if args.len() != enum_info.decl.type_params.len() {
                        return None;
                    }
                    let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                    let handle = enum_info
                        .variants
                        .values()
                        .flat_map(|variant| &variant.payloads)
                        .find_map(|payload| {
                            let payload_ty = substitute_type(&payload.ty, &substitutions);
                            self.opaque_handle_in_type_inner(&payload_ty, visiting)
                        });
                    visiting.remove(&key);
                    return handle;
                }

                None
            }
            // Callable comparison is rejected before structural opaque-handle
            // inspection, and closures are not cloneable values. Function
            // parameter and result types are call contracts, not values
            // retained inside a capture-free code pointer.
            Type::Closure { .. }
            | Type::Function { .. }
            | Type::TypeParam(_)
            | Type::Module(_)
            | Type::Unit => None,
        }
    }

    fn reject_rng_duplication(
        &self,
        operation: &str,
        ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        // These observers transfer a non-repeatable result out of their
        // unique Task right. Their lowering must move rather than clone; the
        // duplication check still applies to random.Rng, which is not
        // Transfer at all.
        let transfers_unique_task_result = matches!(
            operation,
            "Task.result"
                | "Task.result_or_none"
                | "Task.result_or"
                | "wait_any"
                | "wait_all"
                | "select"
        );
        let operation = if operation.contains('`') {
            operation.to_string()
        } else {
            format!("`{operation}`")
        };
        if !transfers_unique_task_result {
            if let Some(result_ty) = self.nonrepeatable_task_result_in(ty) {
                return Err(Diagnostic::coded_at(
                    "AU3009",
                    span,
                    format!(
                        "cannot use {operation} because duplicating `{ty}` would create a second observation right for non-repeatable task result `{result_ty}`"
                    ),
                )
                .with_help(
                    "transfer the unique Task handle instead; only copy-result tasks and synchronized Queue or repeatable Task results may have multiple observers",
                ));
            }
        }
        if !transfers_unique_task_result {
            if let Some(closure_ty) = self.noncloneable_closure_in_type(ty) {
                return Err(Diagnostic::coded_at(
                    "AU3007",
                    span,
                    format!(
                        "cannot use {operation} because duplicating `{ty}` would duplicate non-cloneable closure `{closure_ty}`"
                    ),
                )
                .with_help(
                    "move the closure so its environment has one owner, or use a named function value when no capture is required",
                ));
            }
        }
        if let Some(handle_ty) = self.opaque_handle_in_type(ty) {
            return Err(Diagnostic::coded_at(
                "AU3007",
                span,
                format!(
                    "cannot use {operation} because duplicating `{ty}` would duplicate opaque FFI handle `{handle_ty}`"
                ),
            )
            .with_help(
                "move the value so the opaque handle keeps one owner; use a consuming `pop`, `remove`, or replacement operation when extracting it from a collection",
            ));
        }
        let qualifier = match self.rng_clone_safety(ty) {
            RngCloneSafety::Safe => return Ok(()),
            RngCloneSafety::ContainsRng => "contains",
            RngCloneSafety::Unknown => {
                let params = rng_clone_obligation_params_in_context_with_modules(
                    ty,
                    self.classes,
                    self.enums,
                    self.imported_modules,
                    self.module_registry,
                );
                if !params.is_empty() {
                    let foreign = params
                        .iter()
                        .filter(|name| {
                            !self.type_params.contains_key(*name) && name.as_str() != "Self"
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !foreign.is_empty() {
                        return Err(Diagnostic::coded_at(
                            "AU3007",
                            span,
                            format!(
                                "cannot prove clone safety for unresolved type parameter{} {} while checking {operation}",
                                if foreign.len() == 1 { "" } else { "s" },
                                foreign
                                    .iter()
                                    .map(|name| format!("`{}`", name))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ));
                    }
                    self.rng_clone_obligations.borrow_mut().extend(params);
                    return Ok(());
                }
                "may contain"
            }
        };
        Err(
            Diagnostic::coded_at(
                "AU3007",
                span,
                format!(
                    "cannot use {operation} because `{ty}` {qualifier} non-cloneable `random.Rng` state"
                ),
            )
            .with_help(
                "move or remove the value so it has one owner, or construct an independent generator with an explicit seed",
            ),
        )
    }

    fn enforce_rng_clone_obligations(
        &self,
        operation: &str,
        obligations: &BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
        span: crate::diag::Span,
    ) -> Result<()> {
        for type_param in obligations {
            let resolved = substitutions
                .get(type_param)
                .cloned()
                .unwrap_or(Type::TypeParam(type_param.clone()));
            self.reject_rng_duplication(operation, &resolved, span)?;
        }
        Ok(())
    }

    fn enforce_array_equality_obligations(
        &self,
        operation: &str,
        obligations: &BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
        span: crate::diag::Span,
    ) -> Result<()> {
        for type_param in obligations {
            let resolved = substitutions
                .get(type_param)
                .cloned()
                .unwrap_or(Type::TypeParam(type_param.clone()));
            self.require_array_equality_eligible(
                &resolved,
                format!("cannot use {operation} with `{resolved}`"),
                span,
            )?;
        }
        Ok(())
    }

    fn enforce_rng_clone_obligations_before_method_inference(
        &self,
        operation: &str,
        obligations: &BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
        method_type_params: &[String],
        span: crate::diag::Span,
    ) -> Result<()> {
        for type_param in obligations {
            if !substitutions.contains_key(type_param)
                && method_type_params
                    .iter()
                    .any(|candidate| candidate == type_param)
            {
                continue;
            }
            let resolved = substitutions
                .get(type_param)
                .cloned()
                .unwrap_or_else(|| Type::TypeParam(type_param.clone()));
            self.reject_rng_duplication(operation, &resolved, span)?;
        }
        Ok(())
    }

    fn enforce_resolved_rng_clone_obligations_before_method_inference(
        &self,
        operation: &str,
        obligations: &[Type],
        method_type_params: &[String],
        span: crate::diag::Span,
    ) -> Result<()> {
        for ty in obligations {
            if matches!(
                ty,
                Type::TypeParam(name)
                    if method_type_params.iter().any(|candidate| candidate == name)
            ) {
                continue;
            }
            self.reject_rng_duplication(operation, ty, span)?;
        }
        Ok(())
    }

    fn enforce_resolved_array_equality_obligations_before_method_inference(
        &self,
        operation: &str,
        obligations: &[Type],
        span: crate::diag::Span,
    ) -> Result<()> {
        for ty in obligations {
            self.require_array_equality_eligible(
                ty,
                format!("cannot use {operation} with `{ty}`"),
                span,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_method_type_substitutions(
        &self,
        operation: &str,
        method_type_params: &[String],
        param_types: &[Type],
        type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        rng_clone_safe_type_params: &BTreeSet<String>,
        array_equality_safe_type_params: &BTreeSet<String>,
        actual_types: &[Type],
        mut substitutions: HashMap<String, Type>,
        span: crate::diag::Span,
    ) -> Result<HashMap<String, Type>> {
        for (expected, actual) in param_types.iter().zip(actual_types) {
            if let Err(error) = unify_type_pattern(expected, actual, &mut substitutions) {
                return Err(Diagnostic::at(
                    span,
                    format!("argument type mismatch for {operation}: {}", error.message),
                ));
            }
        }

        for type_param in method_type_params {
            if !substitutions.contains_key(type_param) {
                return Err(Diagnostic::at(
                    span,
                    format!("cannot infer type parameter `{type_param}` for {operation}"),
                ));
            }
        }

        for (type_param, bounds) in type_param_bounds {
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
            operation,
            rng_clone_safe_type_params,
            &substitutions,
            span,
        )?;
        self.enforce_array_equality_obligations(
            operation,
            array_equality_safe_type_params,
            &substitutions,
            span,
        )?;
        Ok(substitutions)
    }

    fn check_builtin_argument_type(
        &self,
        argument: &Argument,
        expected: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        label: &str,
    ) -> Result<()> {
        let actual = self.type_of_expr_hint(&argument.value, locals, Some(expected))?;
        if actual != *expected {
            return Err(Diagnostic::at(
                argument.span,
                format!("`{}` expects `{}`, found `{}`", label, expected, actual),
            ));
        }
        Ok(())
    }

    fn reject_builtin_argument_sibling_overlap(
        &self,
        args: &[Argument],
        ordered_args: &[Option<&Argument>],
        locals: &HashMap<String, LocalBinding>,
        passing_at: impl Fn(usize) -> Option<ReceiverKind>,
        name_at: impl Fn(usize) -> Option<&'static str>,
    ) -> Result<()> {
        let mut retained = Vec::new();
        for source_argument in args {
            let Some(index) = ordered_args.iter().position(|candidate| {
                candidate.is_some_and(|candidate| std::ptr::eq(candidate, source_argument))
            }) else {
                continue;
            };
            let Some(passing) = passing_at(index) else {
                continue;
            };
            let label = name_at(index).unwrap_or("argument");

            let mut current_accesses = Vec::new();
            self.collect_expr_borrowed_places(
                &source_argument.value,
                locals,
                &mut current_accesses,
            )?;
            self.collect_expr_consumed_places(
                &source_argument.value,
                locals,
                &mut current_accesses,
            )?;
            self.collect_expr_place_reads(
                &source_argument.value,
                locals,
                "argument read",
                &mut current_accesses,
            );

            let direct_access = if let Some(path) = self.borrow_call_place(&source_argument.value) {
                let retained_by_call = passing != ReceiverKind::Value
                    || self
                        .place_path_type(&path, locals, source_argument.span)?
                        .is_some_and(|ty| !self.is_copy_type(&ty));
                retained_by_call.then_some(BorrowedCallPlace {
                    path,
                    passing,
                    param_name: format!("parameter `{label}`"),
                    origin_span: source_argument.span,
                })
            } else {
                None
            };
            if let Some(direct) = &direct_access {
                current_accesses.retain(|access| {
                    access.path != direct.path || access.origin_span != direct.origin_span
                });
                current_accesses.push(direct.clone());
            }

            self.reject_retained_access_overlap(&retained, &current_accesses)?;
            if let Some(direct) = direct_access {
                retained.push(direct);
            }
        }
        Ok(())
    }

    fn reject_builtin_function_argument_sibling_overlap(
        &self,
        builtin: BuiltinFunction,
        args: &[Argument],
        ordered_args: &[Option<&Argument>],
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.reject_builtin_argument_sibling_overlap(
            args,
            ordered_args,
            locals,
            |index| builtin.argument_passing(index),
            |index| builtin.argument_name(index),
        )
    }

    fn reject_builtin_member_argument_sibling_overlap(
        &self,
        member: BuiltinMember,
        args: &[Argument],
        locals: &HashMap<String, LocalBinding>,
        span: crate::diag::Span,
    ) -> Result<()> {
        if args.len() < 2 {
            return Ok(());
        }
        // TaskGroup start calls bind forwarded arguments against the selected
        // target function below. The builtin's variadic shape cannot bind
        // those target-specific names, and its only fixed input is a callable
        // rather than a mutable value place.
        if matches!(
            member,
            BuiltinMember::TaskGroupStart
                | BuiltinMember::TaskGroupStartSoon
                | BuiltinMember::TaskGroupStartWithStack
                | BuiltinMember::TaskGroupStartSoonWithStack
        ) {
            return Ok(());
        }
        let ordered_args = member.bind_args(args, span)?;
        self.reject_builtin_argument_sibling_overlap(
            args,
            &ordered_args,
            locals,
            |index| member.argument_passing(index),
            |index| member.argument_name(index),
        )
    }

    fn reject_builtin_associated_argument_sibling_overlap(
        &self,
        associated: BuiltinAssociatedFunction,
        args: &[Argument],
        ordered_args: &[Option<&Argument>],
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.reject_builtin_argument_sibling_overlap(
            args,
            ordered_args,
            locals,
            |index| associated.argument_passing(index),
            |index| associated.argument_name(index),
        )
    }

    fn check_optional_builtin_timeout_argument(
        &self,
        ordered_args: &[Option<&Argument>],
        index: usize,
        locals: &mut HashMap<String, LocalBinding>,
        label: &str,
    ) -> Result<()> {
        if let Some(argument) = ordered_args.get(index).copied().flatten() {
            self.check_builtin_argument_type(argument, &Type::named("Duration"), locals, label)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        module_name: &'a str,
        type_names: &'a TypeDefinitions,
        type_arities: &'a BTreeMap<String, usize>,
        canonical_type_names: &'a BTreeMap<String, String>,
        classes: &'a BTreeMap<String, ClassInfo>,
        enums: &'a BTreeMap<String, EnumInfo>,
        functions: &'a BTreeMap<String, FunctionInfo>,
        constants: &'a BTreeMap<String, ConstantInfo>,
        traits: &'a BTreeMap<String, TraitInfo>,
        trait_impls: &'a [TraitImplInfo],
        imported_modules: &'a BTreeMap<String, ModuleNamespace>,
        module_registry: &'a BTreeMap<String, ModuleNamespace>,
    ) -> Self {
        static EMPTY_EXTERN_FUNCTIONS: std::sync::OnceLock<BTreeMap<String, ExternFunctionInfo>> =
            std::sync::OnceLock::new();
        static EMPTY_OPAQUE_HANDLES: std::sync::OnceLock<BTreeMap<String, OpaqueHandleInfo>> =
            std::sync::OnceLock::new();
        Self {
            root_module_name: module_name,
            module_name,
            type_names,
            type_arities,
            canonical_type_names,
            classes,
            enums,
            functions,
            constants,
            extern_functions: EMPTY_EXTERN_FUNCTIONS.get_or_init(BTreeMap::new),
            opaque_handles: EMPTY_OPAQUE_HANDLES.get_or_init(BTreeMap::new),
            traits,
            trait_impls,
            imported_modules,
            module_registry,
            current_return_type: None,
            type_params: BTreeMap::new(),
            type_param_bounds: BTreeMap::new(),
            implicit_borrowed_params: BTreeMap::new(),
            active_match_borrow_places: Rc::new(RefCell::new(Vec::new())),
            rng_clone_obligations: Rc::new(RefCell::new(BTreeSet::new())),
            array_equality_obligations: Rc::new(RefCell::new(BTreeSet::new())),
            expr_result_entries: Rc::new(RefCell::new(HashMap::new())),
            closure_owner: ClosureOwner::TopLevel,
            closure_infos: Rc::new(RefCell::new(BTreeMap::new())),
            comprehension_infos: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    fn with_ffi(
        mut self,
        extern_functions: &'a BTreeMap<String, ExternFunctionInfo>,
        opaque_handles: &'a BTreeMap<String, OpaqueHandleInfo>,
    ) -> Self {
        self.extern_functions = extern_functions;
        self.opaque_handles = opaque_handles;
        self
    }

    fn with_return_type(&self, return_type: Type) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
            classes: self.classes,
            enums: self.enums,
            functions: self.functions,
            constants: self.constants,
            extern_functions: self.extern_functions,
            opaque_handles: self.opaque_handles,
            traits: self.traits,
            trait_impls: self.trait_impls,
            imported_modules: self.imported_modules,
            module_registry: self.module_registry,
            current_return_type: Some(return_type),
            type_params: self.type_params.clone(),
            type_param_bounds: self.type_param_bounds.clone(),
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_places: self.active_match_borrow_places.clone(),
            rng_clone_obligations: self.rng_clone_obligations.clone(),
            array_equality_obligations: self.array_equality_obligations.clone(),
            expr_result_entries: self.expr_result_entries.clone(),
            closure_owner: self.closure_owner.clone(),
            closure_infos: self.closure_infos.clone(),
            comprehension_infos: self.comprehension_infos.clone(),
        }
    }

    fn with_type_params(
        &self,
        type_params: BTreeMap<String, ()>,
        type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    ) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
            classes: self.classes,
            enums: self.enums,
            functions: self.functions,
            constants: self.constants,
            extern_functions: self.extern_functions,
            opaque_handles: self.opaque_handles,
            traits: self.traits,
            trait_impls: self.trait_impls,
            imported_modules: self.imported_modules,
            module_registry: self.module_registry,
            current_return_type: self.current_return_type.clone(),
            type_params,
            type_param_bounds,
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_places: self.active_match_borrow_places.clone(),
            rng_clone_obligations: self.rng_clone_obligations.clone(),
            array_equality_obligations: self.array_equality_obligations.clone(),
            expr_result_entries: self.expr_result_entries.clone(),
            closure_owner: self.closure_owner.clone(),
            closure_infos: self.closure_infos.clone(),
            comprehension_infos: self.comprehension_infos.clone(),
        }
    }

    fn with_module_name(&self, module_name: &'a str) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
            classes: self.classes,
            enums: self.enums,
            functions: self.functions,
            constants: self.constants,
            extern_functions: self.extern_functions,
            opaque_handles: self.opaque_handles,
            traits: self.traits,
            trait_impls: self.trait_impls,
            imported_modules: self.imported_modules,
            module_registry: self.module_registry,
            current_return_type: self.current_return_type.clone(),
            type_params: self.type_params.clone(),
            type_param_bounds: self.type_param_bounds.clone(),
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_places: self.active_match_borrow_places.clone(),
            rng_clone_obligations: self.rng_clone_obligations.clone(),
            array_equality_obligations: self.array_equality_obligations.clone(),
            expr_result_entries: self.expr_result_entries.clone(),
            closure_owner: self.closure_owner.clone(),
            closure_infos: self.closure_infos.clone(),
            comprehension_infos: self.comprehension_infos.clone(),
        }
    }

    fn with_rng_clone_obligation_sink(&self, sink: Rc<RefCell<BTreeSet<String>>>) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
            classes: self.classes,
            enums: self.enums,
            functions: self.functions,
            constants: self.constants,
            extern_functions: self.extern_functions,
            opaque_handles: self.opaque_handles,
            traits: self.traits,
            trait_impls: self.trait_impls,
            imported_modules: self.imported_modules,
            module_registry: self.module_registry,
            current_return_type: self.current_return_type.clone(),
            type_params: self.type_params.clone(),
            type_param_bounds: self.type_param_bounds.clone(),
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_places: self.active_match_borrow_places.clone(),
            rng_clone_obligations: sink,
            array_equality_obligations: self.array_equality_obligations.clone(),
            expr_result_entries: self.expr_result_entries.clone(),
            closure_owner: self.closure_owner.clone(),
            closure_infos: self.closure_infos.clone(),
            comprehension_infos: self.comprehension_infos.clone(),
        }
    }

    fn with_array_equality_obligation_sink(&self, sink: Rc<RefCell<BTreeSet<String>>>) -> Self {
        let mut checker = self.with_module_name(self.module_name);
        checker.array_equality_obligations = sink;
        checker
    }

    fn with_closure_owner(mut self, owner: ClosureOwner) -> Self {
        self.closure_owner = owner;
        self
    }

    fn with_implicit_param_borrows(
        mut self,
        params: &[Param],
        param_types: &[Type],
        param_passings: &[ReceiverKind],
    ) -> Self {
        self.implicit_borrowed_params = params
            .iter()
            .zip(param_types)
            .zip(param_passings)
            .filter(|((param, _), passing)| {
                param.mode == ParamMode::Default && **passing == ReceiverKind::Borrow
            })
            .map(|((param, ty), _)| (param.name.clone(), ty.clone()))
            .collect();
        self
    }

    fn peel_specialization<'b>(&self, expr: &'b Expr) -> (&'b Expr, Option<&'b [TypeRef]>) {
        match &expr.kind {
            ExprKind::Group(inner) => self.peel_specialization(inner),
            ExprKind::Specialize { expr, type_args } => {
                (grouped_expr(expr), Some(type_args.as_slice()))
            }
            _ => (expr, None),
        }
    }

    fn lower_explicit_type_args(&self, type_args: &[TypeRef]) -> Result<Vec<Type>> {
        type_args
            .iter()
            .map(|type_arg| {
                lower_type(
                    type_arg,
                    self.type_names,
                    self.type_arities,
                    self.canonical_type_names,
                    &self.type_params,
                )
            })
            .collect()
    }

    fn explicit_type_substitutions(
        &self,
        type_params: &[String],
        type_args: &[TypeRef],
        span: crate::diag::Span,
        callee_name: &str,
    ) -> Result<HashMap<String, Type>> {
        let lowered = self.lower_explicit_type_args(type_args)?;
        if lowered.len() != type_params.len() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "{} expects {} type argument{}, found {}",
                    callee_name,
                    type_params.len(),
                    if type_params.len() == 1 { "" } else { "s" },
                    lowered.len()
                ),
            ));
        }
        Ok(substitutions_from_decl_type_args(type_params, &lowered))
    }

    fn validate_integer_literal(
        &self,
        value: u128,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let Some(bounds) = integer_type_bounds(target_ty) else {
            return Ok(());
        };
        if !IntegerValue::from_literal(value).fits_bounds(bounds) {
            return Err(Diagnostic::at(
                span,
                format!(
                    "integer literal `{}` does not fit in `{}`",
                    value, target_ty
                ),
            ));
        }
        Ok(())
    }

    fn validate_negative_integer_literal(
        &self,
        value: u128,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let Some(bounds) = integer_type_bounds(target_ty) else {
            return Ok(());
        };
        let Some(negative) = IntegerValue::from_literal(value).checked_neg() else {
            return Err(Diagnostic::at(
                span,
                format!(
                    "integer literal `-{}` does not fit in `{}`",
                    value, target_ty
                ),
            ));
        };
        if !negative.fits_bounds(bounds) {
            return Err(Diagnostic::at(
                span,
                format!(
                    "integer literal `-{}` does not fit in `{}`",
                    value, target_ty
                ),
            ));
        }
        Ok(())
    }

    fn validate_float_context_integer_literal(
        &self,
        value: u128,
        negative: bool,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let integer = IntegerValue::from_literal(value);
        let exactly_representable = match target_ty {
            Type::Named(name, args) if args.is_empty() && name == "float32" => {
                integer.to_exact_f32().is_some()
            }
            Type::Named(name, args) if args.is_empty() && name == "float64" => {
                integer.to_exact_f64().is_some()
            }
            _ => return Ok(()),
        };
        if exactly_representable {
            return Ok(());
        }

        let rendered_value = if negative {
            format!("-{}", value)
        } else {
            value.to_string()
        };
        Err(Diagnostic::coded_at(
            "AU2002",
            span,
            format!(
                "integer literal `{}` cannot be represented exactly as `{}`; write an explicit float spelling such as `{}.0` or use `.to_float()` when rounding is intended",
                rendered_value, target_ty, rendered_value
            ),
        ))
    }

    fn merge_control_flow_moves(
        &self,
        locals: &mut HashMap<String, LocalBinding>,
        branch_states: &[&HashMap<String, LocalBinding>],
    ) {
        let binding_names = locals.keys().cloned().collect::<Vec<_>>();
        for name in binding_names {
            let moved = branch_states.iter().any(|state| {
                state
                    .get(&name)
                    .map(|binding| binding.moved)
                    .unwrap_or(false)
            });
            let merged_ty = branch_states
                .iter()
                .filter_map(|state| state.get(&name))
                .map(|binding| binding.ty.clone())
                .reduce(|left, right| merge_type_callable_contracts(&left, &right));
            if let Some(binding) = locals.get_mut(&name) {
                if let Some(merged_ty) = merged_ty {
                    binding.ty = merged_ty;
                }
                binding.moved = moved;
                binding.moved_at = branch_states.iter().find_map(|state| {
                    state
                        .get(&name)
                        .filter(|binding| binding.moved)
                        .and_then(|binding| binding.moved_at)
                });
                binding.moved_fields = branch_states
                    .iter()
                    .filter_map(|state| state.get(&name))
                    .flat_map(|binding| {
                        binding
                            .moved_fields
                            .iter()
                            .map(|(path, span)| (path.clone(), *span))
                    })
                    .collect();
                binding.stale_match_borrow_place = branch_states.iter().find_map(|state| {
                    state
                        .get(&name)
                        .and_then(|binding| binding.stale_match_borrow_place.clone())
                });
            }
        }
    }

    fn const_bool_value(&self, expr: &Expr) -> Option<bool> {
        match &expr.kind {
            ExprKind::Bool(value) => Some(*value),
            ExprKind::Group(inner) => self.const_bool_value(inner),
            ExprKind::Unary {
                op: UnaryOp::Not,
                expr: inner,
            } => self.const_bool_value(inner).map(|value| !value),
            _ => None,
        }
    }

    fn reject_loop_carried_moves(
        &self,
        locals: &HashMap<String, LocalBinding>,
        body_locals: &HashMap<String, LocalBinding>,
        loop_kind: &str,
        span: crate::diag::Span,
    ) -> Result<()> {
        for (name, binding) in locals {
            if self.is_copy_type(&binding.ty) {
                continue;
            }
            let Some(body_binding) = body_locals.get(name) else {
                continue;
            };
            if !binding.moved && body_binding.moved {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "`{}` loop body moves `{}` and may execute more than once",
                        loop_kind, name
                    ),
                ));
            }
            if body_binding
                .moved_fields
                .keys()
                .any(|field| !binding.moved_fields.contains_key(field))
            {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "`{}` loop body partially moves `{}` and may execute more than once",
                        loop_kind, name
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_function(&self, function_info: &FunctionInfo) -> Result<()> {
        let function = &function_info.decl;
        self.validate_view_return_contract(function)?;
        let type_param_scope = type_param_scope(&function.type_params);
        let type_param_bounds = lower_trait_bounds(
            &function.type_param_bounds,
            self.traits,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &type_param_scope,
        )?;
        let return_type = lower_type(
            &function.return_type,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &type_param_scope,
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_closure_owner(ClosureOwner::Function(function.name.clone()))
            .with_implicit_param_borrows(
                &function.params,
                &function_info.signature.params,
                &function_info.signature.param_passings,
            );
        checker.check_param_defaults(
            &function.params,
            &type_param_scope,
            None,
            true,
            "function",
        )?;
        let mut locals = HashMap::new();
        checker.seed_module_scope(&mut locals);
        checker.seed_imported_modules(&mut locals);
        for ((param, ty), passing) in function
            .params
            .iter()
            .zip(function_info.signature.params.iter())
            .zip(function_info.signature.param_passings.iter().copied())
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

        let flow = checker.check_block(&function.body, &mut locals, &return_type, 0, true)?;
        if return_type != Type::Unit && flow != BlockFlow::AlwaysReturns {
            return Err(Diagnostic::at(
                function.span,
                format!("function `{}` is missing a return", function.name),
            ));
        }

        Ok(())
    }

    fn check_method(&self, class_decl: &ClassDecl, method_info: &MethodInfo) -> Result<()> {
        let method = &method_info.decl;
        self.validate_view_return_contract(method)?;
        let class_type_param_scope = type_param_scope(&class_decl.type_params);
        let class_self_type = Type::Named(
            class_decl.name.clone(),
            class_decl
                .type_params
                .iter()
                .cloned()
                .map(Type::TypeParam)
                .collect(),
        );
        let method_type_param_scope =
            merged_type_param_scope(&class_type_param_scope, &method.type_params);
        let class_type_param_bounds = self
            .classes
            .get(&class_decl.name)
            .map(|class_info| class_info.type_param_bounds.clone())
            .unwrap_or_default();
        let type_param_bounds = merge_trait_bounds(
            &class_type_param_bounds,
            &lower_trait_bounds_with_self(
                &method.type_param_bounds,
                self.traits,
                self.type_names,
                self.type_arities,
                self.canonical_type_names,
                &method_type_param_scope,
                Some(&class_self_type),
            )?,
        );
        let return_type = lower_type_with_self(
            &method.return_type,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &method_type_param_scope,
            Some(&class_self_type),
        )?;
        let checker = self
            .with_type_params(method_type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_closure_owner(ClosureOwner::ClassMethod {
                class_name: class_decl.name.clone(),
                method_name: method.name.clone(),
            })
            .with_implicit_param_borrows(
                &method.params,
                &method_info.signature.params,
                &method_info.signature.param_passings,
            );
        checker.check_param_defaults(
            &method.params,
            &method_type_param_scope,
            Some(&class_self_type),
            true,
            "method",
        )?;
        let mut locals = HashMap::new();
        checker.seed_module_scope(&mut locals);
        checker.seed_imported_modules(&mut locals);
        if let Some(receiver_kind) = method.receiver {
            locals.insert(
                "self".to_string(),
                LocalBinding {
                    ty: Type::Named(
                        class_decl.name.clone(),
                        class_decl
                            .type_params
                            .iter()
                            .cloned()
                            .map(Type::TypeParam)
                            .collect(),
                    ),
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

    fn check_top_level(&self, body: &[Stmt]) -> Result<()> {
        let mut locals = HashMap::new();
        self.seed_module_scope(&mut locals);
        self.check_block(body, &mut locals, &Type::Unit, 0, false)?;
        Ok(())
    }

    /// Recognizes the compiler-known `for` iterable forms `enumerate(xs)` and
    /// `zip(xs, ys)`. A user definition of either name shadows the loop form.
    fn loop_form<'e>(&self, iterable: &'e Expr) -> Result<Option<LoopForm<'e>>> {
        let ExprKind::Call { callee, args } = &iterable.kind else {
            return Ok(None);
        };
        let ExprKind::Name(name) = &callee.kind else {
            return Ok(None);
        };
        let kind = match name.as_str() {
            "enumerate" => LoopFormKind::Enumerate,
            "zip" => LoopFormKind::Zip,
            _ => return Ok(None),
        };
        if self.functions.contains_key(name)
            || self.resolve_class_info(name).is_some()
            || self.resolve_enum_info(name).is_some()
        {
            return Ok(None);
        }
        let arity = kind.arity();
        if args.len() != arity {
            return Err(Diagnostic::coded_at(
                "AU2004",
                iterable.span,
                format!(
                    "`{}` takes {} iterable{}, found {}",
                    name,
                    arity,
                    if arity == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
        }
        if let Some(named) = args.iter().find(|argument| argument.name.is_some()) {
            return Err(Diagnostic::coded_at(
                "AU2004",
                named.span,
                format!("`{}` takes positional iterables only", name),
            ));
        }
        Ok(Some(LoopForm {
            kind,
            name: kind.name(),
            span: iterable.span,
            iterables: args.iter().map(|argument| &argument.value).collect(),
        }))
    }

    /// Checks `for ... in enumerate(xs):` and `for ... in zip(xs, ys):`. Both
    /// iterate index-addressable collections in lockstep over the bare-loop
    /// borrow default, so neither accepts an ownership modifier.
    fn check_lockstep_for(
        &self,
        for_stmt: &crate::ast::ForStmt,
        form: LoopForm<'_>,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<()> {
        if for_stmt.borrow_mode.is_some() {
            return Err(Diagnostic::coded_at(
                "AU3002",
                for_stmt.span,
                format!(
                    "`{}` iterates over the bare-loop shared default; write `for ... in {}(...):` without an ownership modifier",
                    form.name, form.name
                ),
            ));
        }

        let mut element_types = Vec::with_capacity(form.iterables.len() + 1);
        if form.kind == LoopFormKind::Enumerate {
            element_types.push(Type::named("int64"));
        }
        let mut any_non_copy = false;
        for iterable in &form.iterables {
            let iterable_ty = self.type_of_expr(iterable, locals)?;
            let Some(element_ty) = lockstep_element_type(&iterable_ty) else {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    iterable.span,
                    format!(
                        "`{}` requires a `list[T]` or `set[T]` iterable, found `{}`",
                        form.name, iterable_ty
                    ),
                )
                .with_help(
                    "these loop forms read collections by position; iterate a `Range` or `Queue[T]` with the bare `for` form",
                ));
            };
            any_non_copy = any_non_copy || !self.is_copy_type(&element_ty);
            element_types.push(element_ty);
        }

        let binding_ty = Type::Tuple(element_types);
        let binding_passing = if any_non_copy {
            ReceiverKind::Borrow
        } else {
            ReceiverKind::Value
        };

        let mut body_locals = locals.clone();
        for iterable in &form.iterables {
            if let Some(place) = self.borrowed_iterable_place(iterable, locals)? {
                let root = place.root.clone();
                if let Some(binding) = body_locals.get_mut(&root) {
                    if place.is_root() {
                        binding.assignable = false;
                        binding.mutable_place = false;
                        if binding.passing == ReceiverKind::Value {
                            binding.passing = ReceiverKind::Borrow;
                            binding.borrow_origin = Some(root);
                        }
                    }
                    binding.frozen_places.insert(place.clone(), iterable.span);
                }
            }
        }

        self.bind_target(
            &for_stmt.target,
            &binding_ty,
            binding_passing,
            false,
            &mut body_locals,
            "loop",
        )?;
        self.check_block(
            &for_stmt.body,
            &mut body_locals,
            return_type,
            loop_depth + 1,
            allow_return,
        )?;
        self.reject_loop_carried_moves(locals, &body_locals, "for", for_stmt.span)?;
        let baseline_locals = locals.clone();
        self.merge_control_flow_moves(locals, &[&baseline_locals, &body_locals]);
        Ok(())
    }

    /// Establish one comprehension clause with the exact same iterable and
    /// target rules as an ordinary bare `for` loop. The returned scope keeps
    /// every place-backed source frozen through all following filters,
    /// clauses, and output evaluation.
    fn bind_comprehension_clause(
        &self,
        clause: &ComprehensionClause,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<(HashMap<String, LocalBinding>, ComprehensionClauseInfo)> {
        self.reject_mutable_returned_view_value(&clause.iterable, locals, false)?;
        let (binding_type, binding_passing, receive_owned, sources) = if let Some(form) =
            self.loop_form(&clause.iterable)?
        {
            let mut element_types = Vec::with_capacity(form.iterables.len() + 1);
            if form.kind == LoopFormKind::Enumerate {
                element_types.push(Type::named("int64"));
            }
            let mut any_non_copy = false;
            for iterable in &form.iterables {
                let iterable_ty = self.type_of_expr(iterable, locals)?;
                let Some(element_ty) = lockstep_element_type(&iterable_ty) else {
                    return Err(Diagnostic::coded_at(
                            "AU2002",
                            iterable.span,
                            format!(
                                "`{}` requires a `list[T]` or `set[T]` iterable, found `{}`",
                                form.name, iterable_ty
                            ),
                        )
                        .with_help(
                            "these comprehension forms read collections by position; use a plain clause for a `Range` or `Queue[T]`",
                        ));
                };
                any_non_copy = any_non_copy || !self.is_copy_type(&element_ty);
                element_types.push(element_ty);
            }
            (
                Type::Tuple(element_types),
                if any_non_copy {
                    ReceiverKind::Borrow
                } else {
                    ReceiverKind::Value
                },
                false,
                form.iterables,
            )
        } else {
            let iterable_ty = self.type_of_expr(&clause.iterable, locals)?;
            let (binding_type, binding_passing, receive_owned) = match &iterable_ty {
                    Type::Named(name, _) if name == "Range" => {
                        (Type::named("int64"), ReceiverKind::Value, false)
                    }
                    Type::Named(name, args) if name == "Queue" && args.len() == 1 => {
                        (args[0].clone(), ReceiverKind::Value, true)
                    }
                    Type::Named(name, args)
                        if matches!(name.as_str(), "list" | "set") && args.len() == 1 =>
                    {
                        let element_ty = args[0].clone();
                        let passing = if self.is_copy_type(&element_ty) {
                            ReceiverKind::Value
                        } else {
                            ReceiverKind::Borrow
                        };
                        (element_ty, passing, false)
                    }
                    _ => {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            clause.iterable.span,
                            format!(
                                "comprehension iteration requires a `Range`, `Queue[T]`, `list[T]`, or `set[T]` iterable, found `{iterable_ty}`"
                            ),
                        ))
                    }
                };
            (
                binding_type,
                binding_passing,
                receive_owned,
                vec![&clause.iterable],
            )
        };

        let mut clause_locals = locals.clone();
        if !receive_owned {
            for source in sources {
                if let Some(place) = self.borrowed_iterable_place(source, locals)? {
                    let root = place.root.clone();
                    if let Some(binding) = clause_locals.get_mut(&root) {
                        if place.is_root() {
                            binding.assignable = false;
                            binding.mutable_place = false;
                            if binding.passing == ReceiverKind::Value {
                                binding.passing = ReceiverKind::Borrow;
                                binding.borrow_origin = Some(root);
                            }
                        }
                        binding.frozen_places.insert(place, source.span);
                    }
                }
            }
        }
        self.bind_target(
            &clause.target,
            &binding_type,
            binding_passing,
            false,
            &mut clause_locals,
            "comprehension",
        )?;
        Ok((
            clause_locals,
            ComprehensionClauseInfo {
                binding_type,
                receive_owned,
            },
        ))
    }

    fn type_comprehension_clauses(
        &self,
        output: &ComprehensionOutput,
        clauses: &[ComprehensionClause],
        clause_index: usize,
        locals: &mut HashMap<String, LocalBinding>,
        expected_output: &[Type],
        clause_infos: &mut Vec<ComprehensionClauseInfo>,
    ) -> Result<Vec<Type>> {
        let clause = clauses.get(clause_index).ok_or_else(|| {
            Diagnostic::new("internal error: comprehension has no iteration clause")
        })?;
        // Evaluation of this iterable happens before entering its own repeated
        // body. In a nested clause it is still part of the surrounding
        // clause's body, so the outer loop-carried-move check sees it.
        let (mut body_locals, clause_info) = self.bind_comprehension_clause(clause, locals)?;
        let loop_baseline = locals.clone();
        clause_infos.push(clause_info);

        for filter in &clause.filters {
            self.reject_mutable_returned_view_value(filter, &mut body_locals, false)?;
            let filter_ty =
                self.type_of_expr_hint(filter, &mut body_locals, Some(&Type::named("bool")))?;
            if filter_ty != Type::named("bool") {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    filter.span,
                    format!("comprehension filter must have type `bool`, found `{filter_ty}`"),
                ));
            }
            self.consume_direct_read_expr(filter, &mut body_locals)?;
        }

        let result_types = if clause_index + 1 < clauses.len() {
            self.type_comprehension_clauses(
                output,
                clauses,
                clause_index + 1,
                &mut body_locals,
                expected_output,
                clause_infos,
            )?
        } else {
            let mut check_output = |value: &Expr,
                                    expected: Option<&Type>,
                                    label: &str|
             -> Result<Type> {
                self.reject_mutable_returned_view_value(value, &mut body_locals, false)?;
                // A comprehension always stores each produced value into a
                // fresh owned collection. This transfer happens even when the
                // collection itself is immediately observed through shared
                // access.
                let actual = match self.type_expr_consuming_result(
                    value,
                    &mut body_locals,
                    expected,
                ) {
                    Ok(actual) => actual,
                    Err(mut diagnostic)
                        if diagnostic.code == "AU3002"
                            && diagnostic.message.starts_with("cannot move") =>
                    {
                        let clone_supported = !diagnostic.edits.is_empty();
                        diagnostic.help.clear();
                        diagnostic.help.push(if clone_supported {
                                "comprehensions store owned values; call `.clone()` on this shared value, or use an explicit consuming loop when the source should be transferred"
                                    .to_string()
                            } else {
                                "comprehensions store owned values and cannot transfer this shared non-cloneable value; receive an owned value from a `Queue`, or use an explicit consuming loop"
                                    .to_string()
                            });
                        return Err(diagnostic);
                    }
                    Err(diagnostic) => return Err(diagnostic),
                };
                if type_contains_closure_value(&actual) {
                    return Err(Diagnostic::coded_at(
                            "AU2002",
                            value.span,
                            "capturing closures cannot be stored in collection literals in this language version",
                        )
                        .with_help(
                            "keep the closure in an immutable local and call it directly, or use a named function or capture-free lambda",
                        ));
                }
                if let Some(expected) = expected {
                    if actual != *expected {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            value.span,
                            format!("{label} has type `{actual}`, expected `{expected}`"),
                        ));
                    }
                    Ok(merge_type_callable_contracts(expected, &actual))
                } else {
                    Ok(actual)
                }
            };
            match output {
                ComprehensionOutput::List(value) => vec![check_output(
                    value,
                    expected_output.first(),
                    "list comprehension result",
                )?],
                ComprehensionOutput::Set(value) => vec![check_output(
                    value,
                    expected_output.first(),
                    "set comprehension result",
                )?],
                ComprehensionOutput::Map { key, value } => vec![
                    check_output(key, expected_output.first(), "map comprehension key")?,
                    check_output(value, expected_output.get(1), "map comprehension value")?,
                ],
            }
        };

        self.reject_loop_carried_moves(&loop_baseline, &body_locals, "comprehension", clause.span)?;
        self.merge_control_flow_moves(locals, &[&loop_baseline, &body_locals]);
        Ok(result_types)
    }

    fn type_of_comprehension(
        &self,
        output: &ComprehensionOutput,
        clauses: &[ComprehensionClause],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let expected_output = match (output, expected) {
            (ComprehensionOutput::List(_), Some(Type::Named(name, args)))
                if name == "list" && args.len() == 1 =>
            {
                args.clone()
            }
            (ComprehensionOutput::Set(_), Some(Type::Named(name, args)))
                if name == "set" && args.len() == 1 =>
            {
                args.clone()
            }
            (ComprehensionOutput::Map { .. }, Some(Type::Named(name, args)))
                if name == "dict" && args.len() == 2 =>
            {
                args.clone()
            }
            _ => Vec::new(),
        };
        let mut clause_infos = Vec::with_capacity(clauses.len());
        let output_types = self.type_comprehension_clauses(
            output,
            clauses,
            0,
            locals,
            &expected_output,
            &mut clause_infos,
        )?;
        let result_type = match output {
            ComprehensionOutput::List(_) => Type::Named(
                "list".to_string(),
                vec![erase_type_callable_contracts(&output_types[0])],
            ),
            ComprehensionOutput::Set(_) => {
                self.require_array_equality_eligible(
                    &output_types[0],
                    format!("cannot use `{}` as a set element", output_types[0]),
                    span,
                )?;
                Type::Named(
                    "set".to_string(),
                    vec![erase_type_callable_contracts(&output_types[0])],
                )
            }
            ComprehensionOutput::Map { .. } => {
                self.require_array_equality_eligible(
                    &output_types[0],
                    format!("cannot use `{}` as a dict key", output_types[0]),
                    span,
                )?;
                Type::Named(
                    "dict".to_string(),
                    vec![
                        erase_type_callable_contracts(&output_types[0]),
                        erase_type_callable_contracts(&output_types[1]),
                    ],
                )
            }
        };
        let id = ComprehensionId::new(self.module_name, self.closure_owner.clone(), span);
        self.comprehension_infos.borrow_mut().insert(
            id.clone(),
            ComprehensionInfo {
                id,
                result_type: result_type.clone(),
                clauses: clause_infos,
            },
        );
        Ok(result_type)
    }

    fn bind_target(
        &self,
        target: &crate::ast::BindingTarget,
        ty: &Type,
        passing: ReceiverKind,
        mutable_place: bool,
        locals: &mut HashMap<String, LocalBinding>,
        context: &str,
    ) -> Result<()> {
        match target {
            crate::ast::BindingTarget::Name { name, span } => {
                if locals.contains_key(name) {
                    return Err(Diagnostic::at(
                        *span,
                        format!(
                            "{} binding `{}` would shadow an existing name",
                            context, name
                        ),
                    ));
                }
                let leaf_passing = if passing == ReceiverKind::BorrowMut {
                    ReceiverKind::BorrowMut
                } else if self.is_copy_type(ty) {
                    ReceiverKind::Value
                } else {
                    passing
                };
                locals.insert(
                    name.clone(),
                    LocalBinding {
                        ty: ty.clone(),
                        assignable: false,
                        mutable_place: mutable_place && leaf_passing == ReceiverKind::BorrowMut,
                        managed_resource: false,
                        passing: leaf_passing,
                        borrow_origin: None,
                        borrowed_at: (leaf_passing != ReceiverKind::Value).then_some(*span),
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
                Ok(())
            }
            crate::ast::BindingTarget::Tuple { elements, span } => {
                if passing == ReceiverKind::BorrowMut {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        *span,
                        "`mut` tuple targets are not supported; bind the tuple to one mutable name and update its elements explicitly",
                    ));
                }
                let Type::Tuple(element_types) = ty else {
                    return Err(Diagnostic::at(
                        *span,
                        format!("tuple binding requires a tuple value, found `{}`", ty),
                    ));
                };
                if elements.len() != element_types.len() {
                    return Err(Diagnostic::at(
                        *span,
                        format!(
                            "tuple binding has {} elements but the value has {}",
                            elements.len(),
                            element_types.len()
                        ),
                    ));
                }
                for (element, element_ty) in elements.iter().zip(element_types) {
                    self.bind_target(element, element_ty, passing, false, locals, context)?;
                }
                Ok(())
            }
        }
    }

    fn check_destructure(
        &self,
        destructure: &crate::ast::DestructureStmt,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let value_ty = self.type_of_expr(&destructure.value, locals)?;
        self.reject_owned_view_value(&destructure.value, locals, "a destructured owned value")?;
        let mut candidate = locals.clone();
        self.bind_target(
            &destructure.target,
            &value_ty,
            ReceiverKind::Value,
            false,
            &mut candidate,
            "tuple",
        )?;
        self.consume_value_expr(&destructure.value, locals)?;
        self.bind_target(
            &destructure.target,
            &value_ty,
            ReceiverKind::Value,
            false,
            locals,
            "tuple",
        )
    }

    fn check_block(
        &self,
        body: &[Stmt],
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        let mut flow = BlockFlow::FallsThrough;

        for (stmt_index, stmt) in body.iter().enumerate() {
            self.expire_views_before(statement_span(stmt), locals);
            match stmt {
                Stmt::Assign(assign) => {
                    let transferred_closure_loans = match &assign.target {
                        AssignTarget::Name(target)
                            if grouped_name(&assign.value)
                                .is_some_and(|source| target != source) =>
                        {
                            let source = grouped_name(&assign.value)
                                .expect("guarded grouped closure move")
                                .to_string();
                            locals
                                .get(&source)
                                .filter(|binding| !binding.closure_loans.is_empty())
                                .map(|binding| (source, binding.closure_loans.clone()))
                        }
                        _ => None,
                    };
                    self.check_assign(assign, locals)?;
                    let assigned_value = grouped_expr(&assign.value);
                    if let (
                        AssignTarget::Name(binding_name),
                        ExprKind::Lambda {
                            captures: Some(_), ..
                        },
                    ) = (&assign.target, &assigned_value.kind)
                    {
                        let id = ClosureId::new(
                            self.module_name,
                            self.closure_owner.clone(),
                            assigned_value.span,
                        );
                        let info = self.closure_infos.borrow().get(&id).cloned();
                        if let Some(info) = info {
                            let last_use = body
                                .iter()
                                .skip(stmt_index + 1)
                                .rev()
                                .find_map(|stmt| {
                                    last_name_reference_span_in_stmt(stmt, binding_name)
                                })
                                .unwrap_or(assign.span);
                            let mut loans = Vec::new();
                            for capture in info.captures.iter().filter(|capture| {
                                matches!(
                                    capture.mode,
                                    ClosureCaptureMode::SharedView
                                        | ClosureCaptureMode::MutableView
                                )
                            }) {
                                let kind = if capture.mode == ClosureCaptureMode::MutableView {
                                    crate::ast::ViewKind::Mutable
                                } else {
                                    crate::ast::ViewKind::Shared
                                };
                                let source = self.canonicalize_view_place(
                                    PlacePath::root(capture.name.clone()),
                                    locals,
                                );
                                let parent = locals
                                    .get(&capture.name)
                                    .and_then(|binding| binding.view.as_ref())
                                    .map(|_| capture.name.as_str());
                                self.ensure_view_loan_available(
                                    &source,
                                    kind,
                                    parent,
                                    capture.span,
                                    locals,
                                )?;
                                let ancestors = self.view_ancestor_names(parent, locals);
                                loans.push(ViewBinding {
                                    kind,
                                    source,
                                    parent: parent.map(str::to_string),
                                    ancestors,
                                    created_at: capture.span,
                                    last_use,
                                });
                            }
                            if let Some(binding) = locals.get_mut(binding_name) {
                                binding.closure_loans = loans;
                            }
                        }
                    } else if let (
                        AssignTarget::Name(binding_name),
                        Some((source_name, mut loans)),
                    ) = (&assign.target, transferred_closure_loans)
                    {
                        let last_use = body
                            .iter()
                            .skip(stmt_index + 1)
                            .rev()
                            .find_map(|stmt| last_name_reference_span_in_stmt(stmt, binding_name))
                            .unwrap_or(assign.span);
                        for loan in &mut loans {
                            loan.last_use = last_use;
                        }
                        if let Some(source) = locals.get_mut(&source_name) {
                            source.closure_loans.clear();
                        }
                        if let Some(binding) = locals.get_mut(binding_name) {
                            binding.closure_loans = loans;
                        }
                    }
                }
                Stmt::View(view) => {
                    if locals.contains_key(&view.name) {
                        return Err(Diagnostic::coded_at(
                            "AU3004",
                            view.span,
                            format!("view binding `{}` would shadow an existing name", view.name),
                        ));
                    }
                    let syntactic_place = self.borrow_call_place(&view.source);
                    let direct_parent = syntactic_place.as_ref().and_then(|place| {
                        locals
                            .get(&place.root)
                            .is_some_and(|binding| binding.view.is_some())
                            .then(|| place.root.clone())
                    });
                    let source_place = self.view_place(&view.source, locals)?.ok_or_else(|| {
                        Diagnostic::coded_at(
                            "AU3004",
                            view.source.span,
                            "a view source must be an addressable local, parameter, receiver, fixed field, tuple position, or existing view",
                        )
                    })?;
                    let parent = direct_parent
                        .or_else(|| self.returned_view_call_parent(&view.source, locals));
                    let parent_kind = parent.as_deref().and_then(|name| {
                        locals
                            .get(name)
                            .and_then(|binding| binding.view.as_ref())
                            .map(|view| view.kind)
                    });
                    let returned_kind = self.returned_view_call_kind(&view.source, locals)?;
                    if let Some(returned_kind) = returned_kind {
                        let requested = if view.mutable {
                            crate::ast::ViewKind::Mutable
                        } else {
                            crate::ast::ViewKind::Shared
                        };
                        if requested != returned_kind {
                            return Err(Diagnostic::coded_at(
                                "AU3010",
                                view.span,
                                "a returned view must initialize a view binding with the same shared or mutable kind",
                            ));
                        }
                    }
                    if view.mutable && parent_kind == Some(crate::ast::ViewKind::Shared) {
                        return Err(Diagnostic::coded_at(
                            "AU3004",
                            view.source.span,
                            "a shared view cannot be escalated to a mutable view",
                        ));
                    }
                    if view.mutable
                        && parent_kind != Some(crate::ast::ViewKind::Mutable)
                        && returned_kind != Some(crate::ast::ViewKind::Mutable)
                        && !self.is_mutable_place(&view.source, locals)?
                    {
                        return Err(Diagnostic::coded_at(
                            "AU3004",
                            view.source.span,
                            format!("mutable view source `{source_place}` is not mutable"),
                        ));
                    }
                    let ty = if returned_kind.is_some() {
                        self.type_of_expr(&view.source, locals)?
                    } else {
                        let type_place = syntactic_place.as_ref().unwrap_or(&source_place);
                        self.place_path_type(type_place, locals, view.source.span)?
                            .ok_or_else(|| {
                                Diagnostic::coded_at(
                                    "AU3004",
                                    view.source.span,
                                    "a view source must have a known stable place type",
                                )
                            })?
                    };
                    let kind = if view.mutable {
                        crate::ast::ViewKind::Mutable
                    } else {
                        crate::ast::ViewKind::Shared
                    };
                    self.ensure_view_loan_available(
                        &source_place,
                        kind,
                        parent.as_deref(),
                        view.span,
                        locals,
                    )?;
                    let last_use = body
                        .iter()
                        .skip(stmt_index + 1)
                        .rev()
                        .find_map(|stmt| last_name_reference_span_in_stmt(stmt, &view.name))
                        .unwrap_or(view.span);
                    let passing = if kind == crate::ast::ViewKind::Mutable {
                        ReceiverKind::BorrowMut
                    } else {
                        ReceiverKind::Borrow
                    };
                    let ancestors = self.view_ancestor_names(parent.as_deref(), locals);
                    locals.insert(
                        view.name.clone(),
                        LocalBinding {
                            ty,
                            assignable: view.mutable,
                            mutable_place: view.mutable,
                            managed_resource: false,
                            passing,
                            borrow_origin: Some(source_place.root.clone()),
                            borrowed_at: Some(view.span),
                            match_borrow_place: Some(source_place.clone()),
                            stale_match_borrow_place: None,
                            shared_match_scrutinee: None,
                            moved: false,
                            moved_at: None,
                            moved_fields: BTreeMap::new(),
                            frozen_places: BTreeMap::new(),
                            shared_match_places: BTreeMap::new(),
                            captured: false,
                            view: Some(ViewBinding {
                                kind,
                                source: source_place,
                                parent,
                                ancestors,
                                created_at: view.span,
                                last_use,
                            }),
                            closure_loans: Vec::new(),
                        },
                    );
                }
                Stmt::Destructure(destructure) => self.check_destructure(destructure, locals)?,
                Stmt::Pass(_) => {}
                Stmt::Assert(assert_stmt) => {
                    let condition_ty = self.type_of_expr(&assert_stmt.condition, locals)?;
                    self.reject_mutable_returned_view_value(&assert_stmt.condition, locals, false)?;
                    if condition_ty != Type::named("bool") {
                        return Err(Diagnostic::at(
                            assert_stmt.span,
                            format!(
                                "`assert` condition must have type `bool`, found `{}`",
                                condition_ty
                            ),
                        )
                        .with_help(
                            "Aura has no implicit truthiness; compare the value explicitly, for example `value != 0`",
                        ));
                    }
                    self.consume_direct_read_expr(&assert_stmt.condition, locals)?;

                    if let Some(message) = &assert_stmt.message {
                        let mut message_locals = locals.clone();
                        let message_ty = self.type_of_expr(message, &mut message_locals)?;
                        self.reject_mutable_returned_view_value(
                            message,
                            &mut message_locals,
                            false,
                        )?;
                        if message_ty != Type::named("str") {
                            return Err(Diagnostic::at(
                                assert_stmt.span,
                                format!(
                                    "`assert` message must have type `str`, found `{}`",
                                    message_ty
                                ),
                            ));
                        }
                    }
                }
                Stmt::Expr(expr_stmt) => {
                    self.type_of_expr(&expr_stmt.expr, locals)?;
                    self.reject_mutable_returned_view_value(&expr_stmt.expr, locals, false)?;
                    self.consume_value_expr(&expr_stmt.expr, locals)?;
                }
                Stmt::Return(return_stmt) => {
                    if !allow_return {
                        return Err(Diagnostic::at(
                            return_stmt.span,
                            "`return` is only allowed inside a function body",
                        ));
                    }
                    let view_contract = self.current_view_return();
                    if let Some(contract) = view_contract {
                        let expected_kind = if contract.mutable {
                            crate::ast::ViewKind::Mutable
                        } else {
                            crate::ast::ViewKind::Shared
                        };
                        if return_stmt.view != Some(expected_kind) {
                            return Err(Diagnostic::coded_at(
                                "AU3010",
                                return_stmt.span,
                                format!(
                                    "this function must return a {} view from `{}`",
                                    if contract.mutable {
                                        "mutable"
                                    } else {
                                        "shared"
                                    },
                                    contract.origin
                                ),
                            ));
                        }
                        let value = return_stmt.value.as_ref().ok_or_else(|| {
                            Diagnostic::coded_at(
                                "AU3010",
                                return_stmt.span,
                                "a view return requires an addressable source place",
                            )
                        })?;
                        let returned = self.view_place(value, locals)?.ok_or_else(|| {
                            Diagnostic::coded_at(
                                "AU3010",
                                value.span,
                                "a returned view must derive from its declared receiver or parameter origin",
                            )
                        })?;
                        let source_kind = self
                            .borrow_call_place(value)
                            .and_then(|place| {
                                locals
                                    .get(&place.root)
                                    .and_then(|binding| binding.view.as_ref())
                                    .map(|view| view.kind)
                            })
                            .or(self.returned_view_call_kind(value, locals)?);
                        if expected_kind == crate::ast::ViewKind::Mutable
                            && source_kind == Some(crate::ast::ViewKind::Shared)
                        {
                            return Err(Diagnostic::coded_at(
                                "AU3010",
                                value.span,
                                "a shared view cannot be returned as a mutable view",
                            ));
                        }
                        if returned.root != contract.origin {
                            return Err(Diagnostic::coded_at(
                                "AU3010",
                                value.span,
                                format!(
                                    "returned view derives from `{}`, but the declaration names origin `{}`",
                                    returned.root, contract.origin
                                ),
                            )
                            .with_secondary(contract.span, "returned-view origin is declared here"));
                        }
                    } else if return_stmt.view.is_some() {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            return_stmt.span,
                            "`return view` requires a matching `-> view ... from source` declaration",
                        ));
                    }
                    if view_contract.is_none()
                        && return_stmt.value.as_ref().is_some_and(|value| {
                            self.returned_view_call_kind(value, locals)
                                .ok()
                                .flatten()
                                .is_some()
                        })
                    {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            return_stmt.span,
                            "a returned view cannot satisfy an ordinary owned return",
                        )
                        .with_help(
                            "declare `-> view T from source` and use `return view ...`, or return an explicit owned clone",
                        ));
                    }
                    let ty = if let Some(value) = &return_stmt.value {
                        self.type_of_expr_hint(value, locals, Some(return_type))?
                    } else {
                        Type::Unit
                    };
                    if view_contract.is_none() {
                        if let Some(value) = &return_stmt.value {
                            self.reject_owned_view_value(
                                value,
                                locals,
                                "an ordinary owned return",
                            )?;
                        }
                    }
                    if type_contains_loan_closure(&ty) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            return_stmt.span,
                            "a function cannot return a closure containing a live view",
                        )
                        .with_help(
                            "keep the loan-bearing closure in its matching inferred local and call it directly",
                        ));
                    }
                    if &ty != return_type {
                        return Err(Diagnostic::at(
                            return_stmt.span,
                            format!(
                                "return type mismatch: expected `{}`, found `{}`",
                                return_type, ty
                            ),
                        ));
                    }
                    if view_contract.is_none() {
                        if let Some(value) = &return_stmt.value {
                            self.consume_value_expr(value, locals)?;
                        }
                    }
                    flow = BlockFlow::AlwaysReturns;
                    break;
                }
                Stmt::If(if_stmt) => {
                    let control_last_uses = locals
                        .keys()
                        .filter_map(|name| {
                            last_name_reference_span_in_stmt(stmt, name)
                                .map(|span| (name.clone(), span))
                        })
                        .collect::<BTreeMap<_, _>>();
                    let mut fallthrough_locals = locals.clone();
                    let mut branch_entry_states = Vec::new();
                    for branch in &if_stmt.branches {
                        self.expire_views_before(branch.condition.span, &mut fallthrough_locals);
                        let condition_ty =
                            self.type_of_expr(&branch.condition, &mut fallthrough_locals)?;
                        self.reject_mutable_returned_view_value(
                            &branch.condition,
                            &mut fallthrough_locals,
                            false,
                        )?;
                        if condition_ty != Type::named("bool") {
                            return Err(Diagnostic::at(
                                branch.span,
                                format!(
                                    "`if` condition must have type `bool`, found `{}`",
                                    condition_ty
                                ),
                            ));
                        }
                        branch_entry_states.push(fallthrough_locals.clone());
                    }

                    let mut all_return = true;
                    let mut branch_states = Vec::new();
                    let mut later_branches_reachable = true;
                    for (branch, branch_entry) in
                        if_stmt.branches.iter().zip(branch_entry_states.iter())
                    {
                        let mut branch_locals = branch_entry.clone();
                        self.expire_views_unused_in_branch(
                            &branch.body,
                            &control_last_uses,
                            &mut branch_locals,
                        );
                        let branch_flow = self.check_block(
                            &branch.body,
                            &mut branch_locals,
                            return_type,
                            loop_depth,
                            allow_return,
                        )?;
                        let branch_reachable = later_branches_reachable
                            && self.const_bool_value(&branch.condition) != Some(false);
                        if branch_reachable && branch_flow != BlockFlow::AlwaysReturns {
                            all_return = false;
                            branch_states.push(branch_locals);
                        }
                        if later_branches_reachable
                            && self.const_bool_value(&branch.condition) == Some(true)
                        {
                            later_branches_reachable = false;
                        }
                    }

                    let mut else_state = None;
                    if let Some(else_body) = &if_stmt.else_body {
                        let mut else_locals = fallthrough_locals.clone();
                        self.expire_views_unused_in_branch(
                            else_body,
                            &control_last_uses,
                            &mut else_locals,
                        );
                        let else_flow = self.check_block(
                            else_body,
                            &mut else_locals,
                            return_type,
                            loop_depth,
                            allow_return,
                        )?;
                        if later_branches_reachable && else_flow != BlockFlow::AlwaysReturns {
                            all_return = false;
                            else_state = Some(else_locals);
                        }
                    } else if later_branches_reachable {
                        all_return = false;
                    }

                    if let Some(ref else_locals) = else_state {
                        let states = branch_states
                            .iter()
                            .map(|state| state as &HashMap<String, LocalBinding>)
                            .chain(std::iter::once(
                                else_locals as &HashMap<String, LocalBinding>,
                            ))
                            .collect::<Vec<_>>();
                        self.merge_control_flow_moves(locals, &states);
                    } else {
                        let baseline_locals = fallthrough_locals;
                        let mut states = branch_states
                            .iter()
                            .map(|state| state as &HashMap<String, LocalBinding>)
                            .collect::<Vec<_>>();
                        if later_branches_reachable {
                            states.push(&baseline_locals);
                        }
                        self.merge_control_flow_moves(locals, &states);
                    }

                    if all_return {
                        flow = BlockFlow::AlwaysReturns;
                        break;
                    }
                }
                Stmt::Match(match_stmt) => {
                    self.reject_mutable_returned_view_value(&match_stmt.scrutinee, locals, false)?;
                    let match_flow = self.check_match(
                        match_stmt,
                        locals,
                        return_type,
                        loop_depth,
                        allow_return,
                    )?;
                    if match_flow == BlockFlow::AlwaysReturns {
                        flow = BlockFlow::AlwaysReturns;
                        break;
                    }
                }
                Stmt::For(for_stmt) => {
                    self.reject_mutable_returned_view_value(&for_stmt.iterable, locals, false)?;
                    if let Some(form) = self.loop_form(&for_stmt.iterable)? {
                        self.check_lockstep_for(
                            for_stmt,
                            form,
                            locals,
                            return_type,
                            loop_depth,
                            allow_return,
                        )?;
                        continue;
                    }
                    let iterable_ty = self.type_of_expr(&for_stmt.iterable, locals)?;
                    if matches!(&iterable_ty, Type::Named(name, _) if name == "Range")
                        && for_stmt.borrow_mode.is_some()
                    {
                        return Err(Diagnostic::coded_at(
                            "AU3004",
                            for_stmt.span,
                            "Range iteration yields copy `int64` values, so ownership modifiers have nothing to modify or transfer; use the bare form `for item in range(...):`",
                        ));
                    }
                    if matches!(&iterable_ty, Type::Named(name, args) if name == "Queue" && args.len() == 1)
                        && for_stmt.borrow_mode.is_some()
                    {
                        return Err(Diagnostic::at(
                            for_stmt.span,
                            "Queue iteration receives values; each received item is already owned by the loop binding, and the Queue handle is a copy value, so ownership modifiers have nothing to modify; use the bare form `for item in queue:`",
                        ));
                    }
                    let (binding_ty, binding_passing, binding_mutable_place) =
                        match (&iterable_ty, for_stmt.borrow_mode) {
                        (Type::Named(name, _), _) if name == "Range" => {
                            (Type::named("int64"), ReceiverKind::Value, false)
                        }
                        (Type::Named(name, args), borrow_mode)
                            if name == "Queue" && args.len() == 1 =>
                        {
                            let element_ty = args[0].clone();
                            debug_assert!(borrow_mode.is_none());
                            (element_ty, ReceiverKind::Value, false)
                        }
                        (Type::Named(name, args), borrow_mode) if name == "list" && args.len() == 1 => {
                            if borrow_mode == Some(ReceiverKind::BorrowMut)
                                && !self.is_mutable_place(&for_stmt.iterable, locals)?
                            {
                                return Err(Diagnostic::coded_at(
                                    "AU3002",
                                    for_stmt.iterable.span,
                                    "`for value in mut ...:` requires a mutable `list[T]` place",
                                ));
                            }
                            let element_ty = args[0].clone();
                            let passing = match borrow_mode {
                                Some(ReceiverKind::BorrowMut) => ReceiverKind::BorrowMut,
                                None | Some(ReceiverKind::Borrow)
                                    if !self.is_copy_type(&element_ty) =>
                                {
                                    ReceiverKind::Borrow
                                }
                                _ => ReceiverKind::Value,
                            };
                            (
                                element_ty,
                                passing,
                                passing == ReceiverKind::BorrowMut,
                            )
                        }
                        (Type::Named(name, args), Some(ReceiverKind::BorrowMut))
                            if name == "set" && args.len() == 1 =>
                        {
                            return Err(Diagnostic::at(
                                for_stmt.iterable.span,
                                "`for value in mut ...:` is not supported for `set[T]`; use `add`/`remove` on the set directly",
                            ))
                        }
                        (Type::Named(name, args), borrow_mode) if name == "set" && args.len() == 1 => {
                            let element_ty = args[0].clone();
                            let passing = match borrow_mode {
                                None | Some(ReceiverKind::Borrow)
                                    if !self.is_copy_type(&element_ty) =>
                                {
                                    ReceiverKind::Borrow
                                }
                                _ => ReceiverKind::Value,
                            };
                            (element_ty, passing, false)
                        }
                        _ => {
                            return Err(Diagnostic::at(
                                for_stmt.span,
                                format!(
                                    "`for` currently requires a `Range`, `Queue[T]`, `list[T]`, or `set[T]` iterable, found `{}`",
                                    iterable_ty
                                ),
                            ))
                        }
                    };
                    if for_stmt.borrow_mode == Some(ReceiverKind::BorrowMut) {
                        if let Some(place) =
                            self.borrowed_iterable_place(&for_stmt.iterable, locals)?
                        {
                            self.ensure_place_not_frozen(&place, for_stmt.iterable.span, locals)?;
                        }
                    }
                    if matches!(
                        (&iterable_ty, for_stmt.borrow_mode),
                        (Type::Named(name, args), Some(ReceiverKind::BorrowMut))
                            if name == "list" && args.len() == 1
                    ) && !self.is_mutable_place(&for_stmt.iterable, locals)?
                    {
                        return Err(Diagnostic::coded_at(
                            "AU3002",
                            for_stmt.iterable.span,
                            "`for ... in mut ...` requires a mutable iterable place",
                        ));
                    }
                    if for_stmt.borrow_mode == Some(ReceiverKind::Value)
                        && !self.is_copy_type(&iterable_ty)
                    {
                        self.consume_value_expr(&for_stmt.iterable, locals)?;
                    }
                    let mut body_locals = locals.clone();
                    let effective_borrow_mode = match &iterable_ty {
                        Type::Named(name, args) if name == "Queue" && args.len() == 1 => None,
                        _ => match for_stmt.borrow_mode {
                            Some(ReceiverKind::Value) => None,
                            Some(mode) => Some(mode),
                            None => Some(ReceiverKind::Borrow),
                        },
                    };
                    if let Some(borrow_mode) = effective_borrow_mode {
                        if let Some(place) =
                            self.borrowed_iterable_place(&for_stmt.iterable, locals)?
                        {
                            let root = place.root.clone();
                            if let Some(binding) = body_locals.get_mut(&root) {
                                if place.is_root() {
                                    binding.assignable = false;
                                    binding.mutable_place = false;
                                    if binding.passing == ReceiverKind::Value {
                                        binding.passing = borrow_mode;
                                        binding.borrow_origin = Some(root);
                                    }
                                }
                                binding
                                    .frozen_places
                                    .insert(place.clone(), for_stmt.iterable.span);
                            }
                        }
                    }
                    self.bind_target(
                        &for_stmt.target,
                        &binding_ty,
                        binding_passing,
                        binding_mutable_place,
                        &mut body_locals,
                        "loop",
                    )?;
                    self.check_block(
                        &for_stmt.body,
                        &mut body_locals,
                        return_type,
                        loop_depth + 1,
                        allow_return,
                    )?;
                    self.reject_loop_carried_moves(locals, &body_locals, "for", for_stmt.span)?;
                    let baseline_locals = locals.clone();
                    self.merge_control_flow_moves(locals, &[&baseline_locals, &body_locals]);
                }
                Stmt::With(with_stmt) => {
                    let with_flow =
                        self.check_with(with_stmt, locals, return_type, loop_depth, allow_return)?;
                    if with_flow == BlockFlow::AlwaysReturns {
                        flow = BlockFlow::AlwaysReturns;
                        break;
                    }
                }
                Stmt::While(while_stmt) => {
                    let condition_ty = self.type_of_expr(&while_stmt.condition, locals)?;
                    self.reject_mutable_returned_view_value(&while_stmt.condition, locals, false)?;
                    if condition_ty != Type::named("bool") {
                        return Err(Diagnostic::at(
                            while_stmt.span,
                            format!(
                                "`while` condition must have type `bool`, found `{}`",
                                condition_ty
                            ),
                        ));
                    }
                    let mut body_locals = locals.clone();
                    self.check_block(
                        &while_stmt.body,
                        &mut body_locals,
                        return_type,
                        loop_depth + 1,
                        allow_return,
                    )?;
                    if self.const_bool_value(&while_stmt.condition) != Some(false) {
                        self.reject_loop_carried_moves(
                            locals,
                            &body_locals,
                            "while",
                            while_stmt.span,
                        )?;
                        let baseline_locals = locals.clone();
                        self.merge_control_flow_moves(locals, &[&baseline_locals, &body_locals]);
                    }
                }
                Stmt::Break(break_stmt) => {
                    if loop_depth == 0 {
                        return Err(Diagnostic::at(
                            break_stmt.span,
                            "`break` is only allowed inside a loop",
                        ));
                    }
                }
                Stmt::Continue(continue_stmt) => {
                    if loop_depth == 0 {
                        return Err(Diagnostic::at(
                            continue_stmt.span,
                            "`continue` is only allowed inside a loop",
                        ));
                    }
                }
            }
        }

        Ok(flow)
    }

    fn check_with(
        &self,
        with_stmt: &WithStmt,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        if locals.contains_key(&with_stmt.binding) {
            return Err(Diagnostic::at(
                with_stmt.span,
                format!(
                    "with binding `{}` would shadow an existing name",
                    with_stmt.binding
                ),
            ));
        }

        let value_ty = self.type_of_expr(&with_stmt.value, locals)?;
        self.reject_mutable_returned_view_value(&with_stmt.value, locals, false)?;
        self.reject_owned_view_value(&with_stmt.value, locals, "a managed `with` resource")?;
        self.require_with_resource(&value_ty, with_stmt.span)?;
        self.consume_value_expr(&with_stmt.value, locals)?;

        let mut body_locals = locals.clone();
        body_locals.insert(
            with_stmt.binding.clone(),
            LocalBinding {
                ty: value_ty,
                assignable: true,
                mutable_place: true,
                managed_resource: true,
                passing: ReceiverKind::Value,
                borrow_origin: None,
                borrowed_at: None,
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
        self.check_block(
            &with_stmt.body,
            &mut body_locals,
            return_type,
            loop_depth,
            allow_return,
        )
        .inspect(|&flow| {
            if flow != BlockFlow::AlwaysReturns {
                self.merge_control_flow_moves(locals, &[&body_locals]);
            }
        })
    }

    fn check_assign(
        &self,
        assign: &AssignStmt,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if let AssignTarget::Index { object, index } = &assign.target {
            if assign.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    "`mut` can only be used when introducing a new binding",
                ));
            }

            if assign.annotation.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    "index assignment cannot include a type annotation",
                ));
            }

            if let Some(place) = self.borrow_call_place(object) {
                self.ensure_place_mutation_allowed(&place, assign.span, locals)?;
                let through_view = locals
                    .get(&place.root)
                    .and_then(|binding| binding.view.as_ref())
                    .map(|_| place.root.as_str());
                self.ensure_place_not_locked_by_view(&place, through_view, assign.span, locals)?;
            }
            if !self.is_mutable_place(object, locals)? {
                if self.is_shared_self_place(object, locals) {
                    return Err(self.shared_self_mutation_diagnostic(assign.span, locals));
                }
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign through immutable place `{}`",
                        self.render_index_target(object)
                    ),
                ));
            }

            let object_ty = self.type_of_expr(object, locals)?;
            let locals_before_index = locals.clone();
            self.reject_mutable_returned_view_value(index, locals, false)?;
            let target_ty = if let Some(target_ty) = array_element_type(&object_ty).cloned() {
                self.check_array_index_type(index, locals)?;
                target_ty
            } else if let Some(target_ty) = vec_element_type(&object_ty).cloned() {
                self.check_vec_index_type(index, index.span, locals)?;
                if assign.op.is_some() && !self.is_copy_type(&target_ty) {
                    return Err(Diagnostic::coded_at(
                        "AU3006",
                        assign.span,
                        self.indexed_compound_assignment_guidance("list", "index", &target_ty),
                    ));
                }
                target_ty
            } else if let Some((key_ty, value_ty)) = map_key_value_types(&object_ty) {
                self.require_array_equality_eligible(
                    key_ty,
                    format!("cannot use dict indexing with `{key_ty}`"),
                    index.span,
                )?;
                let index_ty = self.type_of_expr_hint(index, locals, Some(key_ty))?;
                if index_ty != *key_ty {
                    return Err(Diagnostic::at(
                        index.span,
                        format!("map keys must have type `{}`, found `{}`", key_ty, index_ty),
                    ));
                }
                if assign.op.is_some() && !self.is_copy_type(value_ty) {
                    return Err(Diagnostic::coded_at(
                        "AU3006",
                        assign.span,
                        self.indexed_compound_assignment_guidance("dict", "key", value_ty),
                    ));
                }
                value_ty.clone()
            } else {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot index non-Array, list, or dict value `{}`",
                        object_ty
                    ),
                ));
            };

            let retained_target = self
                .retained_place_access(
                    object,
                    &object_ty,
                    ReceiverKind::BorrowMut,
                    "indexed assignment target",
                )
                .into_iter()
                .collect::<Vec<_>>();
            let mut index_borrowed_places = Vec::new();
            self.collect_expr_borrowed_places(
                index,
                &locals_before_index,
                &mut index_borrowed_places,
            )?;
            self.reject_retained_access_overlap(&retained_target, &index_borrowed_places)?;
            self.consume_direct_read_expr(index, locals)?;
            let index_moved_accesses = self.newly_moved_place_accesses(
                &locals_before_index,
                locals,
                "index expression",
                index.span,
            );
            self.reject_retained_access_overlap(&retained_target, &index_moved_accesses)?;
            let locals_before_value = locals.clone();
            let value_ty = self.type_of_expr_hint(&assign.value, locals, Some(&target_ty))?;
            self.reject_mutable_returned_view_value(&assign.value, locals, false)?;
            if assign.op.is_none() {
                self.reject_owned_view_value(
                    &assign.value,
                    locals,
                    "an indexed collection element",
                )?;
            }
            let operator_access = if let Some(op) = assign.op {
                if Self::binary_uses_builtin_value_semantics(op, &target_ty, &value_ty) {
                    None
                } else {
                    self.type_of_binary_operator_via_trait(assign.span, op, &target_ty, &value_ty)?
                }
            } else {
                None
            };
            let rhs_passing = if assign.op.is_some() {
                operator_access
                    .as_ref()
                    .map(|operator| operator.rhs_passing)
                    .unwrap_or(ReceiverKind::Borrow)
            } else {
                ReceiverKind::Value
            };
            self.reject_retained_expr_overlap(
                &retained_target,
                &assign.value,
                &value_ty,
                Some(rhs_passing),
                &locals_before_value,
                locals,
                "indexed assignment value",
            )?;
            let final_value_ty = if let Some(op) = assign.op {
                self.type_of_binary(assign.span, op, target_ty.clone(), value_ty.clone())?
            } else {
                value_ty
            };

            if final_value_ty != target_ty {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign value of type `{}` to indexed element of type `{}`",
                        final_value_ty, target_ty
                    ),
                ));
            }

            if assign.op.is_none() {
                self.consume_value_expr(&assign.value, locals)?;
            } else if let Some(operator) = operator_access {
                self.apply_operator_operand_passing(
                    &assign.value,
                    operator.rhs_passing,
                    "operator right operand",
                    locals,
                )?;
            }
            return Ok(());
        }

        if let AssignTarget::Member { object, field } = &assign.target {
            if assign.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    "`mut` can only be used when introducing a new binding",
                ));
            }

            if assign.annotation.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    "member assignment cannot include a type annotation",
                ));
            }

            if let Some(path) = self.member_target_path(object, field) {
                self.ensure_place_mutation_allowed(&path, assign.span, locals)?;
                let through_view = locals
                    .get(&path.root)
                    .and_then(|binding| binding.view.as_ref())
                    .map(|_| path.root.as_str());
                self.ensure_place_not_locked_by_view(&path, through_view, assign.span, locals)?;
            }
            if !self.is_mutable_place(object, locals)? {
                if self.is_shared_self_place(object, locals) {
                    return Err(self.shared_self_mutation_diagnostic(assign.span, locals));
                }
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign through immutable place `{}`",
                        self.render_member_target(object, field)
                    ),
                ));
            }

            if let Some(path) = self.member_target_path(object, field) {
                if let Some(binding) = locals.get(&path.root) {
                    if assign.op.is_some() && Self::field_path_is_moved(binding, &path.projections)
                    {
                        return Err(Diagnostic::at(
                            assign.span,
                            format!(
                                "cannot read moved field `{}` from `{}` in compound assignment",
                                path.projections, path.root
                            ),
                        ));
                    }
                }
            }

            let target_ty = self.resolve_member_target_type(object, field, assign.span, locals)?;
            let locals_before_value = locals.clone();
            let value_ty = self.type_of_expr_hint(&assign.value, locals, Some(&target_ty))?;
            self.reject_mutable_returned_view_value(&assign.value, locals, false)?;
            if assign.op.is_none() {
                self.reject_owned_view_value(&assign.value, locals, "a class field")?;
            }
            let operator_access = if let Some(op) = assign.op {
                if Self::binary_uses_builtin_value_semantics(op, &target_ty, &value_ty) {
                    None
                } else {
                    self.type_of_binary_operator_via_trait(assign.span, op, &target_ty, &value_ty)?
                }
            } else {
                None
            };
            if assign.op.is_some() {
                let retained_target = self
                    .member_target_path(object, field)
                    .and_then(|path| match &operator_access {
                        Some(operator) => self.retained_path_access(
                            path,
                            &target_ty,
                            operator.receiver_passing,
                            "compound assignment target",
                            assign.span,
                        ),
                        None if !self.is_copy_type(&target_ty) => Some(BorrowedCallPlace {
                            path,
                            passing: ReceiverKind::Borrow,
                            param_name: "compound assignment target".to_string(),
                            origin_span: assign.span,
                        }),
                        None => None,
                    })
                    .into_iter()
                    .collect::<Vec<_>>();
                let rhs_passing = operator_access
                    .as_ref()
                    .map(|operator| operator.rhs_passing)
                    .unwrap_or(ReceiverKind::Borrow);
                self.reject_retained_expr_overlap(
                    &retained_target,
                    &assign.value,
                    &value_ty,
                    Some(rhs_passing),
                    &locals_before_value,
                    locals,
                    "compound assignment value",
                )?;
            }
            let final_value_ty = if let Some(op) = assign.op {
                self.type_of_binary(assign.span, op, target_ty.clone(), value_ty.clone())?
            } else {
                value_ty
            };

            if final_value_ty != target_ty {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign value of type `{}` to member `{}` of type `{}`",
                        final_value_ty,
                        self.render_member_target(object, field),
                        target_ty
                    ),
                ));
            }

            if assign.op.is_none() {
                self.consume_value_expr(&assign.value, locals)?;
            } else if let Some(operator) = operator_access {
                self.apply_operator_operand_passing(
                    &assign.value,
                    operator.rhs_passing,
                    "operator right operand",
                    locals,
                )?;
            }
            if let Some(path) = self.member_target_path(object, field) {
                if let Some(binding) = locals.get_mut(&path.root) {
                    Self::clear_moved_field_path(binding, &path.projections);
                }
                self.invalidate_match_borrow_bindings_for_place(&path, locals);
            }
            return Ok(());
        }

        let binding_name = match &assign.target {
            AssignTarget::Name(name) => name,
            AssignTarget::Member { .. } => unreachable!("handled above"),
            AssignTarget::Index { .. } => unreachable!("handled above"),
        };
        let annotation_ty = assign
            .annotation
            .as_ref()
            .map(|annotation| {
                lower_type(
                    annotation,
                    self.type_names,
                    self.type_arities,
                    self.canonical_type_names,
                    &self.type_params,
                )
            })
            .transpose()?;
        let existing_binding = locals.get(binding_name).cloned();
        if let Some(existing) = &existing_binding {
            self.ensure_pattern_binding_not_stale(binding_name, assign.span, existing)?;
        }
        let existing_ty = existing_binding.as_ref().map(|binding| binding.ty.clone());
        let mut borrow_info_locals = locals.clone();
        let locals_before_value = locals.clone();
        let value_ty = self.type_of_expr_hint(
            &assign.value,
            locals,
            existing_ty.as_ref().or(annotation_ty.as_ref()),
        )?;
        self.reject_mutable_returned_view_value(&assign.value, locals, false)?;
        let direct_view_kind = self.direct_view_value_kind(&assign.value, locals)?;
        let writes_through_existing_mutable_view =
            existing_binding.as_ref().is_some_and(|binding| {
                binding
                    .view
                    .as_ref()
                    .is_some_and(|view| view.kind == crate::ast::ViewKind::Mutable)
                    && self.is_copy_type(&value_ty)
                    && matches!(&grouped_expr(&assign.value).kind, ExprKind::Name(_))
            });
        if direct_view_kind.is_some() && !writes_through_existing_mutable_view {
            return Err(Diagnostic::coded_at(
                "AU3010",
                assign.value.span,
                "a view must initialize an explicit `view` binding",
            )
            .with_help(format!(
                "write `view {binding_name} = ...` with the returned view's declared capability"
            )));
        }

        if let Some(existing) = existing_binding {
            if assign.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "`{}` is already declared; `mut` cannot redeclare an existing binding",
                        binding_name
                    ),
                ));
            }

            if assign.annotation.is_some() && assign.op.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "compound assignment to `{}` cannot include a type annotation",
                        binding_name
                    ),
                ));
            }

            self.ensure_place_mutation_allowed(
                &PlacePath::root(binding_name.clone()),
                assign.span,
                locals,
            )?;
            self.ensure_place_not_locked_by_view(
                &PlacePath::root(binding_name.clone()),
                existing.view.as_ref().map(|_| binding_name.as_str()),
                assign.span,
                locals,
            )?;
            if !existing.assignable && !existing.mutable_place {
                if existing
                    .borrow_origin
                    .as_deref()
                    .is_some_and(|origin| origin.starts_with("module constant `"))
                {
                    return Err(Diagnostic::coded_at(
                        "AU3003",
                        assign.span,
                        format!("module constant `{binding_name}` is immutable"),
                    )
                    .with_help(
                        "put mutable state in a local value owned by `main` or another explicit owner",
                    ));
                }
                if existing.borrow_origin.is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU3003",
                        assign.span,
                        format!(
                            "cannot rebind shared alias `{}`; shared aliases are non-assignable",
                            binding_name
                        ),
                    )
                    .with_help(format!(
                        "use `{}` only for shared access; rebinding requires a separate `mut` value obtained through `own` input or a supported `.clone()`",
                        binding_name
                    )));
                }
                return Err(Diagnostic::coded_at(
                    "AU3003",
                    assign.span,
                    format!(
                        "cannot assign to immutable binding `{}`; declare it with `mut` to rebind it",
                        binding_name
                    ),
                ));
            }

            if existing.moved && assign.op.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot read moved value `{}` in compound assignment",
                        binding_name
                    ),
                ));
            }

            if let Some(annotation_ty) = annotation_ty {
                if annotation_ty != existing.ty {
                    return Err(Diagnostic::at(
                        assign.span,
                        format!(
                            "reassignment annotation for `{}` has type `{}`, expected `{}`",
                            binding_name, annotation_ty, existing.ty
                        ),
                    ));
                }
            }

            let operator_access = if let Some(op) = assign.op {
                if Self::binary_uses_builtin_value_semantics(op, &existing.ty, &value_ty) {
                    None
                } else {
                    self.type_of_binary_operator_via_trait(
                        assign.span,
                        op,
                        &existing.ty,
                        &value_ty,
                    )?
                }
            } else {
                None
            };
            if assign.op.is_some() {
                let retained_target = match &operator_access {
                    Some(operator) => self
                        .retained_path_access(
                            PlacePath::root(binding_name.clone()),
                            &existing.ty,
                            operator.receiver_passing,
                            "compound assignment target",
                            assign.span,
                        )
                        .into_iter()
                        .collect::<Vec<_>>(),
                    None if !self.is_copy_type(&existing.ty) => vec![BorrowedCallPlace {
                        path: PlacePath::root(binding_name.clone()),
                        passing: ReceiverKind::Borrow,
                        param_name: "compound assignment target".to_string(),
                        origin_span: assign.span,
                    }],
                    None => Vec::new(),
                };
                let rhs_passing = operator_access
                    .as_ref()
                    .map(|operator| operator.rhs_passing)
                    .unwrap_or(ReceiverKind::Borrow);
                self.reject_retained_expr_overlap(
                    &retained_target,
                    &assign.value,
                    &value_ty,
                    Some(rhs_passing),
                    &locals_before_value,
                    locals,
                    "compound assignment value",
                )?;
            }

            let final_value_ty = if let Some(op) = assign.op {
                self.type_of_binary(assign.span, op, existing.ty.clone(), value_ty.clone())?
            } else {
                value_ty.clone()
            };

            if final_value_ty != existing.ty {
                if matches!(final_value_ty, Type::Function { .. })
                    && matches!(existing.ty, Type::Function { .. })
                {
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        assign.span,
                        function_type_mismatch_message(&existing.ty, &final_value_ty),
                    ));
                }
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign value of type `{}` to `{}` of type `{}`",
                        final_value_ty, binding_name, existing.ty
                    ),
                ));
            }

            if assign.op.is_none() && !writes_through_existing_mutable_view {
                self.consume_value_expr(&assign.value, locals)?;
            } else if let Some(operator) = operator_access {
                self.apply_operator_operand_passing(
                    &assign.value,
                    operator.rhs_passing,
                    "operator right operand",
                    locals,
                )?;
            }
            if let Some(existing) = locals.get_mut(binding_name) {
                if assign.op.is_none() {
                    // Rebinding may widen the set of runtime call targets even
                    // when their structural function type is unchanged. Keep
                    // only callable metadata shared by both the old and new
                    // values so a later indirect call cannot use a name or
                    // default that some assigned target does not support.
                    existing.ty = merge_type_callable_contracts(&existing.ty, &final_value_ty);
                }
                existing.moved = false;
                existing.moved_at = None;
                existing.moved_fields.clear();
            }
            self.invalidate_match_borrow_bindings_for_place(
                &PlacePath::root(binding_name.clone()),
                locals,
            );
            return Ok(());
        }

        if assign.op.is_some() {
            return Err(Diagnostic::at(
                assign.span,
                format!(
                    "compound assignment requires an existing mutable binding `{}`",
                    binding_name
                ),
            ));
        }

        let contextual_capturing_closure = annotation_ty
            .as_ref()
            .is_some_and(|annotation| closure_signature_matches_function(&value_ty, annotation));
        let mutable_repeatable_closure = matches!(
            value_ty,
            Type::Closure {
                call_kind: ClosureCallKind::MutableRepeatable,
                ..
            }
        );
        if contextual_capturing_closure && mutable_repeatable_closure && !assign.mutable {
            return Err(Diagnostic::coded_at(
                "AU3003",
                assign.span,
                "a mutable-repeatable closure must be stored in a mutable local",
            )
            .with_help(format!(
                "write `mut {binding_name}: ... = lambda [mut ...] ...`"
            )));
        }
        if contextual_capturing_closure && assign.mutable && !mutable_repeatable_closure {
            return Err(Diagnostic::coded_at(
                "AU2002",
                assign.span,
                "a capturing closure cannot be stored in a mutable `def(...) -> ...` binding",
            )
            .with_help(
                "keep the contextualized closure in an immutable local so its capture ownership and call count remain precise",
            ));
        }
        let final_ty = if contextual_capturing_closure {
            value_ty.clone()
        } else {
            annotation_ty.unwrap_or_else(|| value_ty.clone())
        };
        if value_ty != final_ty {
            if let Some(expected) = assign
                .annotation
                .as_ref()
                .and_then(|source| self.written_alias_type(source, &final_ty))
            {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    assign.span,
                    format!("binding `{binding_name}`: expected {expected}, found {value_ty}"),
                ));
            }
            if matches!(value_ty, Type::Function { .. })
                && matches!(final_ty, Type::Function { .. })
            {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    assign.span,
                    function_type_mismatch_message(&final_ty, &value_ty),
                ));
            }
            return Err(Diagnostic::at(
                assign.span,
                format!(
                    "binding `{}` has annotated type `{}`, but value has type `{}`",
                    binding_name, final_ty, value_ty
                ),
            ));
        }

        if let Some(borrowed) = self.expr_borrow_info(&assign.value, &mut borrow_info_locals)? {
            if self.is_copy_type(&final_ty) {
                self.consume_value_expr(&assign.value, locals)?;
                locals.insert(
                    binding_name.clone(),
                    LocalBinding {
                        ty: final_ty,
                        assignable: assign.mutable,
                        mutable_place: assign.mutable,
                        managed_resource: false,
                        passing: ReceiverKind::Value,
                        borrow_origin: None,
                        borrowed_at: None,
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
                return Ok(());
            }
            let rendered_source = {
                let rendered = self.render_place_expr(&assign.value);
                if rendered == "<place>" {
                    borrowed.origin.clone()
                } else {
                    rendered
                }
            };
            if borrowed.passing == ReceiverKind::BorrowMut {
                let mut diagnostic = Diagnostic::coded_at(
                    "AU3002",
                    assign.value.span,
                    format!(
                        "cannot create local alias `{binding_name}` from mutable access to `{rendered_source}`; local mutable aliases do not write through to their source"
                    ),
                )
                .with_help(format!(
                    "mutate `{rendered_source}` directly, or pass it to another `mut` parameter"
                ));
                if let Some(origin) = borrow_info_locals
                    .get(&borrowed.origin)
                    .and_then(|binding| binding.borrowed_at)
                {
                    diagnostic = diagnostic.with_secondary(
                        origin,
                        format!("mutable access to `{}` begins here", borrowed.origin),
                    );
                }
                return Err(diagnostic);
            }
            if borrowed.passing == ReceiverKind::Borrow && assign.mutable {
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    assign.value.span,
                    "shared values cannot be bound with `mut`",
                ));
            }
            locals.insert(
                binding_name.clone(),
                LocalBinding {
                    ty: final_ty,
                    assignable: assign.mutable,
                    mutable_place: borrowed.passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing: borrowed.passing,
                    borrow_origin: Some(borrowed.origin),
                    borrowed_at: Some(assign.value.span),
                    match_borrow_place: borrowed.match_borrow_place,
                    stale_match_borrow_place: None,
                    shared_match_scrutinee: borrowed.shared_match_scrutinee,
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
            return Ok(());
        }

        self.consume_value_expr(&assign.value, locals)?;
        locals.insert(
            binding_name.clone(),
            LocalBinding {
                ty: final_ty,
                assignable: assign.mutable,
                mutable_place: assign.mutable,
                managed_resource: false,
                passing: ReceiverKind::Value,
                borrow_origin: None,
                borrowed_at: None,
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
        Ok(())
    }

    fn type_of_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        self.type_of_expr_hint(expr, locals, None)
    }

    /// Replays an expression from the ownership state that preceded it so the
    /// transfer of its owned result happens in source order with the moves the
    /// expression itself performs.
    ///
    /// A replay only ever runs after `type_of_expr_hint` accepted the same
    /// expression under the same expected type, and typing does not depend on
    /// move state, so every type rule this walk would restate is already
    /// proven. The walk therefore reproduces the accepted result type and
    /// reports ownership diagnostics only; restating the type rules here would
    /// add unreachable branches and a second place for them to drift.
    fn type_expr_consuming_result(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        self.reject_owned_view_value(expr, locals, "an owned aggregate")?;
        match &expr.kind {
            ExprKind::Group(inner) => self.type_expr_consuming_result(inner, locals, expected),
            ExprKind::Cast { expr: inner, .. } => {
                let ty = self.type_of_expr_hint(expr, locals, expected)?;
                self.consume_direct_read_expr(inner, locals)?;
                Ok(ty)
            }
            ExprKind::Try(inner) => {
                let ty = self.type_of_expr_hint(expr, locals, expected)?;
                self.consume_value_expr(inner, locals)?;
                Ok(ty)
            }
            ExprKind::Tuple(elements) => {
                let expected_elements = match expected {
                    Some(Type::Tuple(expected_elements))
                        if expected_elements.len() == elements.len() =>
                    {
                        Some(expected_elements.as_slice())
                    }
                    _ => None,
                };
                let element_types = elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        self.type_expr_consuming_result(
                            element,
                            locals,
                            expected_elements.and_then(|types| types.get(index)),
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some((index, _)) = element_types
                    .iter()
                    .enumerate()
                    .find(|(_, ty)| type_contains_loan_closure(ty))
                {
                    return Err(Diagnostic::coded_at(
                        "AU3010",
                        elements[index].span,
                        "a closure containing a live view cannot be stored in an aggregate",
                    )
                    .with_help(
                        "keep the loan closure in its matching inferred local and call it directly",
                    ));
                }
                Ok(Type::Tuple(element_types))
            }
            ExprKind::List(elements) => {
                let mut element_ty = expected.and_then(vec_element_type).cloned();
                for element in elements {
                    let actual =
                        self.type_expr_consuming_result(element, locals, element_ty.as_ref())?;
                    if type_contains_loan_closure(&actual) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            element.span,
                            "a closure containing a live view cannot be stored in a collection",
                        )
                        .with_help(
                            "keep the loan closure in its matching inferred local and call it directly",
                        ));
                    }
                    element_ty.get_or_insert(actual);
                }
                Ok(Type::Named(
                    "list".to_string(),
                    vec![erase_type_callable_contracts(
                        &element_ty.unwrap_or(Type::Unit),
                    )],
                ))
            }
            ExprKind::Set(elements) => {
                let mut element_ty = expected.and_then(set_element_type).cloned();
                for element in elements {
                    let actual =
                        self.type_expr_consuming_result(element, locals, element_ty.as_ref())?;
                    if type_contains_loan_closure(&actual) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            element.span,
                            "a closure containing a live view cannot be stored in a collection",
                        ));
                    }
                    element_ty.get_or_insert(actual);
                }
                Ok(Type::Named(
                    "set".to_string(),
                    vec![erase_type_callable_contracts(
                        &element_ty.unwrap_or(Type::Unit),
                    )],
                ))
            }
            ExprKind::Map(entries) => {
                if entries.is_empty() {
                    if let Some(Type::Named(name, args)) = expected {
                        if name == "set" && args.len() == 1 {
                            return Ok(Type::Named(
                                "set".to_string(),
                                vec![erase_type_callable_contracts(&args[0])],
                            ));
                        }
                    }
                }
                let mut key_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(key_ty, _)| key_ty.clone());
                let mut value_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(_, value_ty)| value_ty.clone());
                for entry in entries {
                    let actual_key =
                        self.type_expr_consuming_result(&entry.key, locals, key_ty.as_ref())?;
                    if type_contains_loan_closure(&actual_key) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            entry.key.span,
                            "a closure containing a live view cannot be stored in a collection",
                        ));
                    }
                    key_ty.get_or_insert(actual_key);
                    let actual_value =
                        self.type_expr_consuming_result(&entry.value, locals, value_ty.as_ref())?;
                    if type_contains_loan_closure(&actual_value) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            entry.value.span,
                            "a closure containing a live view cannot be stored in a collection",
                        ));
                    }
                    value_ty.get_or_insert(actual_value);
                }
                Ok(Type::Named(
                    "dict".to_string(),
                    vec![
                        erase_type_callable_contracts(&key_ty.unwrap_or(Type::Unit)),
                        erase_type_callable_contracts(&value_ty.unwrap_or(Type::Unit)),
                    ],
                ))
            }
            ExprKind::Comprehension { output, clauses } => {
                self.type_of_comprehension(output, clauses, expr.span, locals, expected)
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.type_of_expr(condition, locals)?;
                let result_ty =
                    self.conditional_result_hint(then_expr, else_expr, locals, expected)?;
                if type_contains_loan_closure(&result_ty) {
                    return Err(Diagnostic::coded_at(
                        "AU3010",
                        expr.span,
                        "a closure containing a live view cannot be selected by a conditional",
                    )
                    .with_help(
                        "keep the loan closure in its matching inferred local and call it directly",
                    ));
                }
                let mut then_locals = locals.clone();
                self.type_expr_consuming_result(then_expr, &mut then_locals, Some(&result_ty))?;
                let mut else_locals = locals.clone();
                self.type_expr_consuming_result(else_expr, &mut else_locals, Some(&result_ty))?;
                self.merge_control_flow_moves(locals, &[&then_locals, &else_locals]);
                Ok(result_ty)
            }
            ExprKind::Member { object, field } if Self::member_projects_branch_result(object) => {
                let (_, member_ty) =
                    self.type_member_result_consuming(object, field, expr.span, locals, None)?;
                Ok(member_ty)
            }
            ExprKind::Match {
                scrutinee,
                capability,
                arms,
            } => {
                let result = self.type_of_match_expr(
                    MatchExprParts {
                        scrutinee,
                        borrow_mode: *capability,
                        arms,
                        span: expr.span,
                    },
                    locals,
                    expected,
                    BranchResultUse::Consumed,
                )?;
                if type_contains_loan_closure(&result) {
                    return Err(Diagnostic::coded_at(
                        "AU3010",
                        expr.span,
                        "a closure containing a live view cannot be selected by a match expression",
                    ));
                }
                Ok(result)
            }
            _ => {
                let ty = self.type_of_expr_hint(expr, locals, expected)?;
                self.consume_value_expr_raw(expr, locals)?;
                Ok(ty)
            }
        }
    }

    fn type_member_result_consuming(
        &self,
        object: &Expr,
        field: &str,
        member_span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_object: Option<&Type>,
    ) -> Result<(Type, Type)> {
        match &object.kind {
            ExprKind::Group(inner) => self.type_member_result_consuming(
                inner,
                field,
                member_span,
                locals,
                expected_object,
            ),
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.type_of_expr(condition, locals)?;
                let object_ty =
                    self.conditional_result_hint(then_expr, else_expr, locals, expected_object)?;

                let mut then_locals = locals.clone();
                let (_, then_member_ty) = self.type_member_result_consuming(
                    then_expr,
                    field,
                    member_span,
                    &mut then_locals,
                    Some(&object_ty),
                )?;

                let mut else_locals = locals.clone();
                self.type_member_result_consuming(
                    else_expr,
                    field,
                    member_span,
                    &mut else_locals,
                    Some(&object_ty),
                )?;
                self.merge_control_flow_moves(locals, &[&then_locals, &else_locals]);
                Ok((object_ty, then_member_ty))
            }
            ExprKind::Match {
                scrutinee,
                capability,
                arms,
            } => {
                let object_ty = self.type_of_match_expr(
                    MatchExprParts {
                        scrutinee,
                        borrow_mode: *capability,
                        arms,
                        span: object.span,
                    },
                    locals,
                    expected_object,
                    BranchResultUse::ProjectedField(ProjectedField {
                        name: field,
                        span: member_span,
                    }),
                )?;
                let member_ty = self.resolve_member_type(&object_ty, field, member_span)?;
                Ok((object_ty, member_ty))
            }
            _ => {
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                if let Some(expected_object) = expected_object {
                    if object_ty != *expected_object {
                        return Ok((
                            object_ty.clone(),
                            self.resolve_member_type(&object_ty, field, member_span)?,
                        ));
                    }
                }
                let member_ty = self.resolve_member_type(&object_ty, field, member_span)?;
                let member_expr = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(object.clone()),
                        field: field.to_string(),
                    },
                    span: member_span,
                };
                if !self.is_payload_free_variant_expr(&member_expr) {
                    self.consume_typed_member_value_expr(
                        &member_expr,
                        object,
                        field,
                        &object_ty,
                        &member_ty,
                        locals,
                    )?;
                }
                Ok((object_ty, member_ty))
            }
        }
    }

    fn type_of_expr_without_move_state(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let mut snapshot = locals.clone();
        for binding in snapshot.values_mut() {
            binding.moved = false;
            binding.moved_at = None;
            binding.moved_fields.clear();
            binding.stale_match_borrow_place = None;
        }
        let saved_entries = std::mem::take(&mut *self.expr_result_entries.borrow_mut());
        let result = self.type_of_expr_hint(expr, &mut snapshot, expected);
        *self.expr_result_entries.borrow_mut() = saved_entries;
        result
    }

    fn is_contextual_none_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Name(name) => name == "None",
            ExprKind::Group(inner) => Self::is_contextual_none_expr(inner),
            _ => false,
        }
    }

    fn is_integer_literal_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Int(_) => true,
            ExprKind::Group(inner) => Self::is_integer_literal_expr(inner),
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr: inner,
            } => matches!(inner.kind, ExprKind::Int(_)),
            _ => false,
        }
    }

    fn is_float_literal_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Float(_) => true,
            ExprKind::Group(inner) => Self::is_float_literal_expr(inner),
            _ => false,
        }
    }

    fn collect_pattern_binding_names(pattern: &Pattern, names: &mut BTreeSet<String>) {
        match pattern {
            Pattern::Or(pattern) => {
                for alternative in &pattern.alternatives {
                    Self::collect_pattern_binding_names(alternative, names);
                }
            }
            Pattern::Binding(binding) => {
                names.insert(binding.name.clone());
            }
            Pattern::Tuple(tuple) => {
                for element in &tuple.elements {
                    Self::collect_pattern_binding_names(element, names);
                }
            }
            Pattern::Variant(variant) => {
                for subpattern in &variant.subpatterns {
                    Self::collect_pattern_binding_names(subpattern, names);
                }
            }
            Pattern::Literal(_) | Pattern::Wildcard(_) => {}
        }
    }

    fn conditional_result_hint(
        &self,
        then_expr: &Expr,
        else_expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        if let Some(expected) = expected {
            return Ok(expected.clone());
        }

        if let ExprKind::Group(inner) = &then_expr.kind {
            return self.conditional_result_hint(inner, else_expr, locals, None);
        }
        if let ExprKind::Group(inner) = &else_expr.kind {
            return self.conditional_result_hint(then_expr, inner, locals, None);
        }
        if let (ExprKind::Tuple(then_elements), ExprKind::Tuple(else_elements)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if then_elements.len() == else_elements.len() {
                let element_types = then_elements
                    .iter()
                    .zip(else_elements)
                    .map(|(then_element, else_element)| {
                        self.conditional_result_hint(then_element, else_element, locals, None)
                    })
                    .collect::<Result<Vec<_>>>()?;
                return Ok(Type::Tuple(element_types));
            }
        }
        if let (ExprKind::List(then_elements), ExprKind::List(else_elements)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if !then_elements.is_empty() && then_elements.len() == else_elements.len() {
                let element_types = then_elements
                    .iter()
                    .zip(else_elements)
                    .map(|(then_element, else_element)| {
                        self.conditional_result_hint(then_element, else_element, locals, None)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(element_ty) = element_types.first() {
                    if element_types.iter().all(|actual| actual == element_ty) {
                        return Ok(Type::Named("list".to_string(), vec![element_ty.clone()]));
                    }
                }
            }
        }
        if let (ExprKind::Set(then_elements), ExprKind::Set(else_elements)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if !then_elements.is_empty() && then_elements.len() == else_elements.len() {
                let element_types = then_elements
                    .iter()
                    .zip(else_elements)
                    .map(|(then_element, else_element)| {
                        self.conditional_result_hint(then_element, else_element, locals, None)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(element_ty) = element_types.first() {
                    if element_types.iter().all(|actual| actual == element_ty) {
                        return Ok(Type::Named("set".to_string(), vec![element_ty.clone()]));
                    }
                }
            }
        }
        if let (ExprKind::Map(then_entries), ExprKind::Map(else_entries)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if !then_entries.is_empty() && then_entries.len() == else_entries.len() {
                let entry_types = then_entries
                    .iter()
                    .zip(else_entries)
                    .map(|(then_entry, else_entry)| {
                        Ok((
                            self.conditional_result_hint(
                                &then_entry.key,
                                &else_entry.key,
                                locals,
                                None,
                            )?,
                            self.conditional_result_hint(
                                &then_entry.value,
                                &else_entry.value,
                                locals,
                                None,
                            )?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some((key_ty, value_ty)) = entry_types.first() {
                    if entry_types.iter().all(|(actual_key, actual_value)| {
                        actual_key == key_ty && actual_value == value_ty
                    }) {
                        return Ok(Type::Named(
                            "dict".to_string(),
                            vec![key_ty.clone(), value_ty.clone()],
                        ));
                    }
                }
            }
        }

        let then_guess = self.type_of_expr_without_move_state(then_expr, locals, None);
        let else_guess = self.type_of_expr_without_move_state(else_expr, locals, None);
        let (then_ty, else_ty) = match (then_guess, else_guess) {
            (Ok(then_ty), Ok(else_ty)) => (then_ty, else_ty),
            (Err(_), Ok(else_ty)) => return Ok(else_ty),
            (Ok(then_ty), Err(_)) => return Ok(then_ty),
            (Err(error), Err(_)) => return Err(error),
        };

        if then_ty == else_ty {
            return Ok(merge_type_callable_contracts(&then_ty, &else_ty));
        }
        let then_adopts_else = self
            .type_of_expr_without_move_state(then_expr, locals, Some(&else_ty))
            .is_ok_and(|actual| actual == else_ty);
        let else_adopts_then = self
            .type_of_expr_without_move_state(else_expr, locals, Some(&then_ty))
            .is_ok_and(|actual| actual == then_ty);
        match (then_adopts_else, else_adopts_then) {
            (true, false) => return Ok(else_ty),
            (false, true) => return Ok(then_ty),
            _ => {}
        }
        if Self::is_contextual_none_expr(then_expr) {
            return Ok(else_ty);
        }
        if Self::is_contextual_none_expr(else_expr) {
            return Ok(then_ty);
        }
        if Self::is_integer_literal_expr(then_expr)
            && (is_float_type(&else_ty) || is_integer_type(&else_ty))
        {
            return Ok(else_ty);
        }
        if Self::is_integer_literal_expr(else_expr)
            && (is_float_type(&then_ty) || is_integer_type(&then_ty))
        {
            return Ok(then_ty);
        }
        if Self::is_float_literal_expr(then_expr)
            && !Self::is_float_literal_expr(else_expr)
            && is_float_type(&else_ty)
        {
            return Ok(else_ty);
        }
        if Self::is_float_literal_expr(else_expr)
            && !Self::is_float_literal_expr(then_expr)
            && is_float_type(&then_ty)
        {
            return Ok(then_ty);
        }
        Ok(then_ty)
    }

    fn equality_operand_hint(
        &self,
        left: &Expr,
        right: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<Type> {
        self.conditional_result_hint(left, right, locals, None).ok()
    }

    fn result_consumption_needs_replay(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member { object, .. } => Self::member_projects_branch_result(object),
            kind => matches!(
                kind,
                ExprKind::Group(_)
                    | ExprKind::Cast { .. }
                    | ExprKind::Tuple(_)
                    | ExprKind::List(_)
                    | ExprKind::Set(_)
                    | ExprKind::Map(_)
                    | ExprKind::Comprehension { .. }
                    | ExprKind::Conditional { .. }
                    | ExprKind::Match { .. }
                    | ExprKind::Try(_)
            ),
        }
    }

    /// A member access only needs branch-aware result consumption when its
    /// object is a branching expression whose owned result is produced by more
    /// than one arm. Ordinary member paths — including module-qualified and
    /// enum-variant paths, which are not value objects at all — keep using the
    /// direct consumption path.
    fn member_projects_branch_result(object: &Expr) -> bool {
        match &object.kind {
            ExprKind::Group(inner) => Self::member_projects_branch_result(inner),
            ExprKind::Conditional { .. } | ExprKind::Match { .. } => true,
            _ => false,
        }
    }

    fn type_of_expr_hint(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        if Self::result_consumption_needs_replay(expr) {
            self.expr_result_entries.borrow_mut().insert(
                expr as *const Expr as usize,
                ExprResultEntry {
                    locals: locals.clone(),
                    expected: expected.cloned(),
                },
            );
        }
        match &expr.kind {
            ExprKind::IsNone { .. } => Err(Diagnostic::at(
                expr.span,
                "None tests require Batch 1 flow checking",
            )),
            ExprKind::Lambda {
                captures,
                params,
                body,
            } => self.type_of_lambda(
                LambdaTypingRequest {
                    explicit_captures: captures.as_deref(),
                    params,
                    body,
                    span: expr.span,
                    expected,
                    callable_context: None,
                },
                locals,
            ),
            ExprKind::Name(name) if name == "None" => {
                if let Some(expected_ty) = expected {
                    if matches!(expected_ty, Type::Named(enum_name, args) if enum_name == "Option" && args.len() == 1)
                    {
                        return Ok(expected_ty.clone());
                    }
                }
                Ok(Type::Unit)
            }
            ExprKind::Name(name) => {
                if let Some(binding) = locals.get(name) {
                    self.ensure_pattern_binding_not_stale(name, expr.span, binding)?;
                    self.ensure_place_readable(
                        &PlacePath::root(name.clone()),
                        binding.view.as_ref().map(|_| name.as_str()),
                        expr.span,
                        locals,
                    )?;
                    if binding.moved {
                        return Err(self.moved_value_diagnostic(name, expr.span, binding));
                    }
                    if !binding.moved_fields.is_empty() {
                        let mut diagnostic = Diagnostic::at(
                            expr.span,
                            format!("use of partially moved value `{}`", name),
                        );
                        if let Some(origin) = binding.moved_fields.values().next().copied() {
                            diagnostic = diagnostic
                                .with_secondary(origin, "field moved here")
                                .with_help(
                                    "use shared access to the field when ownership is not needed, or call `.clone()` before moving it when an independent value is required",
                                );
                        }
                        return Err(diagnostic);
                    }
                    return Ok(binding.ty.clone());
                }
                if let Some(function) = self.resolve_function_info(name) {
                    return self.function_value_type(
                        function,
                        expected,
                        None,
                        expr.span,
                        &format!("function `{name}`"),
                    );
                }
                if self.resolve_extern_function_info(name).is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU2999",
                        expr.span,
                        format!(
                            "extern function `{name}` is direct-call-only and cannot be used as a function value"
                        ),
                    )
                    .with_help(format!(
                        "call `{name}(...)` synchronously, or write a named Aura wrapper when a function value is required"
                    )));
                }
                if self.resolve_opaque_handle_info(name).is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU2005",
                        expr.span,
                        format!(
                            "opaque FFI handle type `{name}` is not a value and cannot be constructed"
                        ),
                    )
                    .with_help(
                        "obtain opaque handles from extern function returns and pass them only according to the declared handle capability",
                    ));
                }
                if let Some(alias) = self
                    .current_module_namespace()
                    .and_then(|namespace| namespace.all_aliases.get(name))
                    .or_else(|| self.type_names.checked_aliases.get(name))
                    .or_else(|| self.type_names.imported_aliases.get(name))
                {
                    if let Type::Named(target, _) = &alias.target {
                        if self.resolve_class_info(target).is_some()
                            || self.resolve_enum_info(target).is_some()
                        {
                            return Ok(alias.target.clone());
                        }
                    }
                }
                if let Some(class_info) = self.resolve_class_info(name) {
                    return Ok(Type::named(self.canonical_class_name(name, class_info)));
                }
                if let Some(enum_info) = self.resolve_enum_info(name) {
                    return Ok(Type::named(self.canonical_enum_info_name(name, enum_info)));
                }
                match name.as_str() {
                    "True" => Err(Diagnostic::coded_at(
                        "AU2005",
                        expr.span,
                        "unknown name `True`; did you mean `true`?",
                    )),
                    "False" => Err(Diagnostic::coded_at(
                        "AU2005",
                        expr.span,
                        "unknown name `False`; did you mean `false`?",
                    )),
                    _ => Err(Diagnostic::at(
                        expr.span,
                        format!("unknown name `{}`", name),
                    )),
                }
            }
            ExprKind::Int(value) => {
                if let Some(target_ty) = expected.filter(|ty| is_float_type(ty)) {
                    self.validate_float_context_integer_literal(
                        *value, false, target_ty, expr.span,
                    )?;
                    Ok(target_ty.clone())
                } else {
                    let target_ty = expected
                        .filter(|ty| is_integer_type(ty))
                        .cloned()
                        .unwrap_or_else(|| Type::named("int64"));
                    self.validate_integer_literal(*value, &target_ty, expr.span)?;
                    Ok(target_ty)
                }
            }
            ExprKind::DurationNanos(_) => Ok(Type::named("Duration")),
            ExprKind::BuiltinOmitted => Err(Diagnostic::at(
                expr.span,
                "internal builtin omitted-default marker cannot be used as a source expression",
            )),
            ExprKind::Float(_) => Ok(expected
                .filter(|ty| is_float_type(ty))
                .cloned()
                .unwrap_or_else(|| Type::named("float64"))),
            ExprKind::Bool(_) => Ok(Type::named("bool")),
            ExprKind::String(_) => Ok(Type::named("str")),
            ExprKind::FString(parts) => {
                for part in parts {
                    match part {
                        crate::ast::FormatPart::Expr(expr) => {
                            self.type_of_expr(expr, locals)?;
                        }
                        crate::ast::FormatPart::Formatted {
                            expr,
                            spec,
                            spec_span,
                        } => {
                            let value_type = self.type_of_expr(expr, locals)?;
                            let parsed = parse_format_spec(spec).map_err(|error| {
                                Diagnostic::coded_at("AU1101", *spec_span, error.message)
                            })?;
                            validate_format_spec_for_type(&parsed, &value_type).map_err(|error| {
                                Diagnostic::coded_at("AU2002", *spec_span, error.message).with_help(
                                    "supported codes are d, f, e, x, X, b, o, %, and s; omit the code for ordinary rendering",
                                )
                            })?;
                        }
                        crate::ast::FormatPart::Literal(_) => {}
                    }
                }
                Ok(Type::named("str"))
            }
            ExprKind::Tuple(elements) => {
                let expected_elements = match expected {
                    Some(Type::Tuple(expected_elements))
                        if expected_elements.len() == elements.len() =>
                    {
                        Some(expected_elements.as_slice())
                    }
                    _ => None,
                };
                let element_types = elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        self.type_of_expr_hint(
                            element,
                            locals,
                            expected_elements.and_then(|types| types.get(index)),
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Type::Tuple(element_types))
            }
            ExprKind::List(elements) => {
                let mut element_ty = expected.and_then(vec_element_type).cloned();
                for element in elements {
                    let has_contextual_element_type = element_ty.is_some();
                    let actual = if let Some(expected_element_ty) = element_ty.as_ref() {
                        self.type_of_expr_hint(element, locals, Some(expected_element_ty))?
                    } else {
                        self.type_of_expr(element, locals)?
                    };
                    if type_contains_loan_closure(&actual) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            element.span,
                            "a closure containing a live view cannot be stored in a collection",
                        )
                        .with_help(
                            "keep the loan closure in its matching inferred local and call it directly",
                        ));
                    }
                    if !has_contextual_element_type && type_contains_closure_value(&actual) {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            element.span,
                            "capturing closures cannot be stored in collection literals in this language version",
                        )
                        .with_help(
                            "keep the closure in an immutable local and call it directly, or use a named function or capture-free lambda",
                        ));
                    }
                    if let Some(expected_element_ty) = element_ty.as_ref() {
                        if actual != *expected_element_ty {
                            return Err(Diagnostic::at(
                                element.span,
                                format!(
                                    "list literal elements must all have type `{}`, found `{}`",
                                    expected_element_ty, actual
                                ),
                            ));
                        }
                        element_ty =
                            Some(merge_type_callable_contracts(expected_element_ty, &actual));
                    } else {
                        element_ty = Some(actual);
                    }
                }
                let Some(element_ty) = element_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "empty list literals require an expected `list[T]` type annotation in the bootstrap compiler",
                    ));
                };
                Ok(Type::Named(
                    "list".to_string(),
                    vec![erase_type_callable_contracts(&element_ty)],
                ))
            }
            ExprKind::Set(elements) => {
                let mut element_ty = expected.and_then(set_element_type).cloned();
                for element in elements {
                    let has_contextual_element_type = element_ty.is_some();
                    let actual = if let Some(expected_element_ty) = element_ty.as_ref() {
                        self.type_of_expr_hint(element, locals, Some(expected_element_ty))?
                    } else {
                        self.type_of_expr(element, locals)?
                    };
                    if type_contains_loan_closure(&actual) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            element.span,
                            "a closure containing a live view cannot be stored in a collection",
                        ));
                    }
                    if !has_contextual_element_type && type_contains_closure_value(&actual) {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            element.span,
                            "capturing closures cannot be stored in collection literals in this language version",
                        )
                        .with_help(
                            "keep the closure in an immutable local and call it directly, or use a named function or capture-free lambda",
                        ));
                    }
                    if let Some(expected_element_ty) = element_ty.as_ref() {
                        if actual != *expected_element_ty {
                            return Err(Diagnostic::at(
                                element.span,
                                format!(
                                    "set literal elements must all have type `{}`, found `{}`",
                                    expected_element_ty, actual
                                ),
                            ));
                        }
                        element_ty =
                            Some(merge_type_callable_contracts(expected_element_ty, &actual));
                    } else {
                        element_ty = Some(actual);
                    }
                }
                let Some(element_ty) = element_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "empty set literals require an expected `set[T]` type annotation in the bootstrap compiler",
                    ));
                };
                self.require_array_equality_eligible(
                    &element_ty,
                    format!("cannot use `{element_ty}` as a set element"),
                    expr.span,
                )?;
                Ok(Type::Named(
                    "set".to_string(),
                    vec![erase_type_callable_contracts(&element_ty)],
                ))
            }
            ExprKind::Map(entries) => {
                if entries.is_empty() {
                    if let Some(Type::Named(name, args)) = expected {
                        if name == "set" && args.len() == 1 {
                            self.require_array_equality_eligible(
                                &args[0],
                                format!("cannot use `{}` as a set element", args[0]),
                                expr.span,
                            )?;
                            return Ok(Type::Named(
                                "set".to_string(),
                                vec![erase_type_callable_contracts(&args[0])],
                            ));
                        }
                    }
                }
                let mut key_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(key_ty, _)| key_ty.clone());
                let mut value_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(_, value_ty)| value_ty.clone());
                for entry in entries {
                    let has_contextual_key_type = key_ty.is_some();
                    let actual_key = if let Some(expected_key_ty) = key_ty.as_ref() {
                        self.type_of_expr_hint(&entry.key, locals, Some(expected_key_ty))?
                    } else {
                        self.type_of_expr(&entry.key, locals)?
                    };
                    if type_contains_loan_closure(&actual_key) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            entry.key.span,
                            "a closure containing a live view cannot be stored in a collection",
                        ));
                    }
                    if !has_contextual_key_type && type_contains_closure_value(&actual_key) {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            entry.key.span,
                            "capturing closures cannot be stored in collection literals in this language version",
                        )
                        .with_help(
                            "keep the closure in an immutable local and call it directly, or use a named function or capture-free lambda",
                        ));
                    }
                    if let Some(expected_key_ty) = key_ty.as_ref() {
                        if actual_key != *expected_key_ty {
                            return Err(Diagnostic::at(
                                entry.key.span,
                                format!(
                                    "map literal keys must all have type `{}`, found `{}`",
                                    expected_key_ty, actual_key
                                ),
                            ));
                        }
                        key_ty = Some(merge_type_callable_contracts(expected_key_ty, &actual_key));
                    } else {
                        key_ty = Some(actual_key);
                    }

                    let has_contextual_value_type = value_ty.is_some();
                    let actual_value = if let Some(expected_value_ty) = value_ty.as_ref() {
                        self.type_of_expr_hint(&entry.value, locals, Some(expected_value_ty))?
                    } else {
                        self.type_of_expr(&entry.value, locals)?
                    };
                    if type_contains_loan_closure(&actual_value) {
                        return Err(Diagnostic::coded_at(
                            "AU3010",
                            entry.value.span,
                            "a closure containing a live view cannot be stored in a collection",
                        ));
                    }
                    if !has_contextual_value_type && type_contains_closure_value(&actual_value) {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            entry.value.span,
                            "capturing closures cannot be stored in collection literals in this language version",
                        )
                        .with_help(
                            "keep the closure in an immutable local and call it directly, or use a named function or capture-free lambda",
                        ));
                    }
                    if let Some(expected_value_ty) = value_ty.as_ref() {
                        if actual_value != *expected_value_ty {
                            return Err(Diagnostic::at(
                                entry.value.span,
                                format!(
                                    "map literal values must all have type `{}`, found `{}`",
                                    expected_value_ty, actual_value
                                ),
                            ));
                        }
                        value_ty = Some(merge_type_callable_contracts(
                            expected_value_ty,
                            &actual_value,
                        ));
                    } else {
                        value_ty = Some(actual_value);
                    }
                }
                let (Some(key_ty), Some(value_ty)) = (key_ty, value_ty) else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "empty map literals require an expected `dict[K, V]` type annotation in the bootstrap compiler",
                    ));
                };
                self.require_array_equality_eligible(
                    &key_ty,
                    format!("cannot use `{key_ty}` as a dict key"),
                    expr.span,
                )?;
                Ok(Type::Named(
                    "dict".to_string(),
                    vec![
                        erase_type_callable_contracts(&key_ty),
                        erase_type_callable_contracts(&value_ty),
                    ],
                ))
            }
            ExprKind::Comprehension { output, clauses } => {
                self.type_of_comprehension(output, clauses, expr.span, locals, expected)
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                let condition_ty = self.type_of_expr(condition, locals)?;
                if condition_ty != Type::named("bool") {
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        condition.span,
                        format!(
                            "conditional expression condition must have type `bool`, found `{}`",
                            condition_ty
                        ),
                    )
                    .with_help("Aura has no implicit truthiness; compare the value explicitly"));
                }

                let result_ty =
                    self.conditional_result_hint(then_expr, else_expr, locals, expected)?;
                let mut then_locals = locals.clone();
                let then_ty =
                    self.type_of_expr_hint(then_expr, &mut then_locals, Some(&result_ty))?;
                if then_ty != result_ty {
                    if capturing_closure_branch_mismatch(&result_ty, &then_ty) {
                        return Err(capturing_closure_branch_diagnostic(
                            "conditional",
                            "branch",
                            then_expr.span,
                        ));
                    }
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        then_expr.span,
                        format!(
                            "conditional expression arm expects `{}`, found `{}`",
                            result_ty, then_ty
                        ),
                    ));
                }

                let mut else_locals = locals.clone();
                let else_ty =
                    self.type_of_expr_hint(else_expr, &mut else_locals, Some(&result_ty))?;
                if else_ty != result_ty {
                    if capturing_closure_branch_mismatch(&result_ty, &else_ty) {
                        return Err(capturing_closure_branch_diagnostic(
                            "conditional",
                            "branch",
                            else_expr.span,
                        ));
                    }
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        else_expr.span,
                        format!(
                            "conditional expression arms must have one type; expected `{}`, found `{}`",
                            result_ty, else_ty
                        ),
                    ));
                }
                self.merge_control_flow_moves(locals, &[&then_locals, &else_locals]);
                Ok(merge_type_callable_contracts(
                    &merge_type_callable_contracts(&result_ty, &then_ty),
                    &else_ty,
                ))
            }
            ExprKind::Match {
                scrutinee,
                capability,
                arms,
            } => self.type_of_match_expr(
                MatchExprParts {
                    scrutinee,
                    borrow_mode: *capability,
                    arms,
                    span: expr.span,
                },
                locals,
                expected,
                BranchResultUse::Inspected,
            ),
            ExprKind::Group(inner) => self.type_of_expr_hint(inner, locals, expected),
            ExprKind::Specialize {
                expr: base,
                type_args,
            } => {
                let lowered = self.lower_explicit_type_args(type_args)?;
                match &base.kind {
                    ExprKind::Name(name) if self.resolve_function_info(name).is_some() => {
                        let function = self.resolve_function_info(name).expect(
                            "function lookup is stable during explicit type argument checking",
                        );
                        self.function_value_type(
                            function,
                            expected,
                            Some(&lowered),
                            expr.span,
                            &format!("function `{name}`"),
                        )
                    }
                    ExprKind::Name(name)
                        if matches!(
                            name.as_str(),
                            "Option"
                                | "Result"
                                | "SendError"
                                | "QueueReceive"
                                | "TaskResult"
                                | "WaitAny"
                                | "WaitAll"
                                | "SelectOutcome"
                        ) =>
                    {
                        self.explicit_builtin_type(name, &lowered, expr.span)
                    }
                    ExprKind::Name(name) if name == "set" => {
                        if lowered.len() != 1 {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "type `set` expects exactly one type argument, found {}",
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named("set".to_string(), lowered))
                    }
                    ExprKind::Name(name) if name == "dict" => {
                        if lowered.len() != 2 {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "type `dict` expects exactly two type arguments, found {}",
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named("dict".to_string(), lowered))
                    }
                    ExprKind::Name(name) if self.resolve_class_info(name).is_some() => {
                        let Some(class) = self.resolve_class_info(name) else {
                            unreachable!(
                                "class lookup is stable during explicit type argument checking"
                            );
                        };
                        if lowered.len() != class.decl.type_params.len() {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "class `{}` expects {} type argument{}, found {}",
                                    name,
                                    class.decl.type_params.len(),
                                    if class.decl.type_params.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    },
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named(self.canonical_class_name(name, class), lowered))
                    }
                    ExprKind::Name(name) if self.resolve_enum_info(name).is_some() => {
                        let Some(enum_info) = self.resolve_enum_info(name) else {
                            unreachable!(
                                "enum lookup is stable during explicit type argument checking"
                            );
                        };
                        if lowered.len() != enum_info.decl.type_params.len() {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "enum `{}` expects {} type argument{}, found {}",
                                    name,
                                    enum_info.decl.type_params.len(),
                                    if enum_info.decl.type_params.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    },
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named(
                            self.canonical_enum_info_name(name, enum_info),
                            lowered,
                        ))
                    }
                    _ => self.type_of_expr_hint(base, locals, expected),
                }
            }
            ExprKind::Cast { expr: value, ty } => {
                let target_ty = lower_type(
                    ty,
                    self.type_names,
                    self.type_arities,
                    self.canonical_type_names,
                    &self.type_params,
                )?;
                let source_ty = if is_float_type(&target_ty) && Self::is_integer_literal_expr(value)
                {
                    // An explicit integer-to-float cast retains its exactness-
                    // checked cast semantics; it is not contextual literal typing.
                    self.type_of_expr(value, locals)?
                } else {
                    self.type_of_expr_hint(value, locals, Some(&target_ty))?
                };
                if !is_numeric_type(&source_ty) || !is_numeric_type(&target_ty) {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "casts are only supported between numeric types, found `{}` and `{}`",
                            source_ty, target_ty
                        ),
                    ));
                }
                Ok(target_ty)
            }
            ExprKind::Unary { op, expr: value } => match op {
                UnaryOp::Not => {
                    let value_ty = self.type_of_expr(value, locals)?;
                    if value_ty == Type::named("bool") {
                        Ok(Type::named("bool"))
                    } else if let Some(operator) =
                        self.type_of_unary_operator_via_trait(expr.span, *op, &value_ty)?
                    {
                        self.apply_operator_operand_passing(
                            value,
                            operator.receiver_passing,
                            "operator receiver",
                            locals,
                        )?;
                        Ok(operator.return_type)
                    } else {
                        Err(Diagnostic::at(
                            expr.span,
                            format!("`not` expects `bool`, found `{}`", value_ty),
                        ))
                    }
                }
                UnaryOp::Neg => {
                    let value_ty = match &value.kind {
                        ExprKind::Int(inner) => {
                            if let Some(target_ty) = expected.filter(|ty| is_float_type(ty)) {
                                self.validate_float_context_integer_literal(
                                    *inner, true, target_ty, expr.span,
                                )?;
                                target_ty.clone()
                            } else {
                                let target_ty = expected
                                    .filter(|ty| is_integer_type(ty))
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("int64"));
                                self.validate_negative_integer_literal(
                                    *inner, &target_ty, expr.span,
                                )?;
                                target_ty
                            }
                        }
                        _ => self.type_of_expr_hint(value, locals, expected)?,
                    };
                    if self.is_opaque_handle_type(&value_ty) {
                        Err(Diagnostic::coded_at(
                            "AU2003",
                            expr.span,
                            format!(
                                "opaque FFI handle `{value_ty}` does not support unary operator `-`; FFI v0 does not expose raw pointer arithmetic"
                            ),
                        )
                        .with_help(
                            "declare a reviewed extern function for the native handle operation instead of manipulating its address",
                        ))
                    } else if is_integer_type(&value_ty) || is_float_type(&value_ty) {
                        Ok(value_ty)
                    } else if let Some(operator) =
                        self.type_of_unary_operator_via_trait(expr.span, *op, &value_ty)?
                    {
                        self.apply_operator_operand_passing(
                            value,
                            operator.receiver_passing,
                            "operator receiver",
                            locals,
                        )?;
                        Ok(operator.return_type)
                    } else {
                        Err(Diagnostic::coded_at(
                            "AU2003",
                            expr.span,
                            format!("unary `-` expects a numeric value, found `{}`", value_ty),
                        ))
                    }
                }
                UnaryOp::BitNot => {
                    let value_ty = self.type_of_expr_hint(value, locals, expected)?;
                    if is_integer_type(&value_ty) {
                        Ok(value_ty)
                    } else {
                        Err(Diagnostic::coded_at(
                            "AU2003",
                            expr.span,
                            format!("unary `~` expects an integer value, found `{value_ty}`"),
                        ))
                    }
                }
            },
            ExprKind::Try(inner) => {
                let current_return_type = self.current_return_type.as_ref().ok_or_else(|| {
                    Diagnostic::at(expr.span, "`try` is only allowed inside a function body")
                })?;
                let inner_ty = self.type_of_expr(inner, locals)?;
                let Type::Named(inner_name, inner_args) = &inner_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("`try` requires a `Result[T, E]`, found `{}`", inner_ty),
                    ));
                };
                if inner_name != "Result" || inner_args.len() != 2 {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("`try` requires a `Result[T, E]`, found `{}`", inner_ty),
                    ));
                }

                let Type::Named(return_name, return_args) = current_return_type else {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "`try` requires the enclosing function to return `Result`, found `{}`",
                            current_return_type
                        ),
                    ));
                };
                if return_name != "Result" || return_args.len() != 2 {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "`try` requires the enclosing function to return `Result`, found `{}`",
                            current_return_type
                        ),
                    ));
                }

                if inner_args[1] != return_args[1]
                    && !self.has_from_conversion(&inner_args[1], &return_args[1], expr.span)?
                {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "`try` error type `{}` does not match enclosing `Result` error type `{}`",
                            inner_args[1], return_args[1]
                        ),
                    ));
                }

                Ok(inner_args[0].clone())
            }
            ExprKind::Membership {
                value,
                container,
                negated: _,
                operator_span,
            } => {
                // The container decides the element type the value must have,
                // but the value is written and evaluated first, so the hint is
                // taken from a speculative pass that leaves move state alone.
                let needle_hint = self
                    .type_of_expr_without_move_state(container, locals, None)
                    .ok()
                    .and_then(|ty| membership_needle_type(&ty));
                let value_ty = self.type_of_expr_hint(value, locals, needle_hint.as_ref())?;
                let container_ty = self.type_of_expr(container, locals)?;
                self.check_membership_operands(
                    &value_ty,
                    &container_ty,
                    value.span,
                    *operator_span,
                )?;
                Ok(Type::named("bool"))
            }
            ExprKind::CompareChain { first, links } => {
                // Each operand is typed once, in source order, and a numeric
                // literal still adopts the type its neighbour establishes, the
                // same way a single comparison does.
                let first_hint = links
                    .first()
                    .and_then(|link| self.chain_operand_hint(first, link, locals));
                let mut left_expr: &Expr = first;
                let mut left_ty = self.type_of_expr_hint(first, locals, first_hint.as_ref())?;
                for link in links {
                    match link.op.as_binary_op() {
                        Some(op) => {
                            // The right operand is typed under the left
                            // operand's type, so only the left operand can
                            // still need to adopt its neighbour's. Every
                            // comparison operator produces `bool`, builtin or
                            // through an operator trait whose declaration
                            // already fixes that return type, so the link needs
                            // no further result check.
                            let locals_before_right = locals.clone();
                            let right_ty =
                                self.type_of_expr_hint(&link.operand, locals, Some(&left_ty))?;
                            if left_ty != right_ty && Self::is_numeric_literal_expr(left_expr) {
                                left_ty =
                                    self.type_of_expr_hint(left_expr, locals, Some(&right_ty))?;
                            }
                            if Self::binary_uses_builtin_value_semantics(op, &left_ty, &right_ty) {
                                let retained_left = self
                                    .retained_place_access(
                                        left_expr,
                                        &left_ty,
                                        ReceiverKind::Borrow,
                                        "left operand",
                                    )
                                    .into_iter()
                                    .collect::<Vec<_>>();
                                self.reject_retained_expr_overlap(
                                    &retained_left,
                                    &link.operand,
                                    &right_ty,
                                    None,
                                    &locals_before_right,
                                    locals,
                                    "right operand",
                                )?;
                            }
                            self.type_of_binary(
                                link.op_span,
                                op,
                                left_ty.clone(),
                                right_ty.clone(),
                            )?;
                            left_ty = right_ty;
                        }
                        None => {
                            let container_ty = self.type_of_expr(&link.operand, locals)?;
                            if let Some(needle_ty) = membership_needle_type(&container_ty) {
                                if left_ty != needle_ty && Self::is_numeric_literal_expr(left_expr)
                                {
                                    left_ty = self.type_of_expr_hint(
                                        left_expr,
                                        locals,
                                        Some(&needle_ty),
                                    )?;
                                }
                            }
                            self.check_membership_operands(
                                &left_ty,
                                &container_ty,
                                left_expr.span,
                                link.op_span,
                            )?;
                            left_ty = container_ty;
                        }
                    }
                    left_expr = &link.operand;
                }
                Ok(Type::named("bool"))
            }
            ExprKind::Binary { op, left, right } => {
                let locals_before = locals.clone();
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let left_ty = self.type_of_expr(left, locals)?;
                    let locals_after_left = locals.clone();
                    let mut right_locals = locals_after_left.clone();
                    let right_ty = self.type_of_expr(right, &mut right_locals)?;
                    let borrow_locals = locals_before.clone();
                    let mut left_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        left,
                        &borrow_locals,
                        &mut left_borrowed_places,
                    )?;
                    let left_moved_places =
                        self.newly_moved_places(&locals_before, &locals_after_left);
                    let mut right_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        right,
                        &borrow_locals,
                        &mut right_borrowed_places,
                    )?;
                    let right_moved_places =
                        self.newly_moved_places(&locals_after_left, &right_locals);
                    self.reject_expr_borrow_move_overlap(
                        &left_borrowed_places,
                        &right_moved_places,
                        expr.span,
                    )?;
                    self.reject_expr_borrow_move_overlap(
                        &right_borrowed_places,
                        &left_moved_places,
                        expr.span,
                    )?;
                    let right_reachable = match op {
                        BinaryOp::And => self.const_bool_value(left) != Some(false),
                        BinaryOp::Or => self.const_bool_value(left) != Some(true),
                        _ => true,
                    };
                    if right_reachable {
                        *locals = right_locals;
                    }
                    return self.type_of_binary(expr.span, *op, left_ty, right_ty);
                }
                let operand_expected = match op {
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Less
                    | BinaryOp::LessEq
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEq => None,
                    _ => expected,
                };
                let contextual_left_expected = if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                    self.equality_operand_hint(left, right, locals)
                } else {
                    None
                };
                let mut left_ty = self.type_of_expr_hint(
                    left,
                    locals,
                    contextual_left_expected.as_ref().or(operand_expected),
                )?;
                let locals_after_left = locals.clone();
                let right_hint = array_element_type(&left_ty).unwrap_or(&left_ty);
                let mut right_ty = self.type_of_expr_hint(right, locals, Some(right_hint))?;
                if let Some(right_element) = array_element_type(&right_ty) {
                    if Self::is_numeric_literal_expr(left) {
                        left_ty = self.type_of_expr_hint(left, locals, Some(right_element))?;
                    }
                }
                if left_ty != right_ty
                    && array_element_type(&right_ty).is_none()
                    && (Self::is_integer_literal_expr(left)
                        || matches!(left.kind, ExprKind::Float(_)))
                {
                    left_ty = self.type_of_expr_hint(left, locals, Some(&right_ty))?;
                }
                if left_ty != right_ty
                    && array_element_type(&left_ty).is_none()
                    && (Self::is_integer_literal_expr(right)
                        || matches!(right.kind, ExprKind::Float(_)))
                {
                    right_ty = self.type_of_expr_hint(right, locals, Some(&left_ty))?;
                }
                if *op == BinaryOp::Pow
                    && is_integer_type(&left_ty)
                    && matches!(
                        &right.kind,
                        ExprKind::Unary {
                            op: UnaryOp::Neg,
                            expr: inner,
                        } if matches!(inner.kind, ExprKind::Int(_))
                    )
                {
                    return Err(Diagnostic::coded_at(
                        "AU2003",
                        right.span,
                        "integer power does not accept a negative exponent",
                    )
                    .with_help(
                        "use explicit floating operands such as `base.to_float() ** exponent.to_float()` for fractional power",
                    ));
                }
                let operator_access =
                    if Self::binary_uses_builtin_value_semantics(*op, &left_ty, &right_ty) {
                        None
                    } else {
                        self.type_of_binary_operator_via_trait(expr.span, *op, &left_ty, &right_ty)?
                    };
                let borrow_locals = locals_before.clone();
                let mut left_borrowed_places = Vec::new();
                self.collect_expr_borrowed_places(left, &borrow_locals, &mut left_borrowed_places)?;
                let mut right_borrowed_places = Vec::new();
                self.collect_expr_borrowed_places(
                    right,
                    &borrow_locals,
                    &mut right_borrowed_places,
                )?;
                let left_moved_places = self.newly_moved_places(&locals_before, &locals_after_left);
                let right_moved_places = self.newly_moved_places(&locals_after_left, locals);
                let retained_left_access = match &operator_access {
                    Some(operator) => self.retained_call_place_access(
                        left,
                        &left_ty,
                        operator.receiver_passing,
                        "left operand",
                    ),
                    None => self.retained_place_access(
                        left,
                        &left_ty,
                        ReceiverKind::Borrow,
                        "left operand",
                    ),
                };
                let retained_left = retained_left_access.into_iter().collect::<Vec<_>>();
                if let Some(operator) = &operator_access {
                    if let Some(access) = self.retained_call_place_access(
                        right,
                        &right_ty,
                        operator.rhs_passing,
                        "right operand",
                    ) {
                        right_borrowed_places.push(access);
                    }
                }
                self.collect_expr_place_reads(
                    right,
                    &borrow_locals,
                    "right operand read",
                    &mut right_borrowed_places,
                );
                self.reject_retained_access_overlap(&retained_left, &right_borrowed_places)?;
                let right_moved_accesses = self.newly_moved_place_accesses(
                    &locals_after_left,
                    locals,
                    "right operand",
                    right.span,
                );
                self.reject_retained_access_overlap(&retained_left, &right_moved_accesses)?;
                self.reject_expr_borrow_move_overlap(
                    &left_borrowed_places,
                    &right_moved_places,
                    expr.span,
                )?;
                self.reject_expr_borrow_move_overlap(
                    &right_borrowed_places,
                    &left_moved_places,
                    expr.span,
                )?;
                if let Some(operator) = operator_access {
                    self.apply_operator_operand_passing(
                        left,
                        operator.receiver_passing,
                        "operator receiver",
                        locals,
                    )?;
                    self.apply_operator_operand_passing(
                        right,
                        operator.rhs_passing,
                        "operator right operand",
                        locals,
                    )?;
                }
                self.type_of_binary(expr.span, *op, left_ty, right_ty)
            }
            ExprKind::Member { object, field } => {
                let (base_object, _) = self.peel_specialization(object);
                let associated_owner = match &base_object.kind {
                    ExprKind::Name(class_name) if !locals.contains_key(class_name) => self
                        .resolve_class_info(class_name)
                        .and_then(|class| class.methods.get(field))
                        .filter(|method| method.decl.receiver.is_none())
                        .map(|_| class_name.clone()),
                    _ => self.qualified_module_item(base_object).and_then(
                        |(module_path, class_name)| {
                            self.module_namespace(&module_path)
                                .and_then(|namespace| namespace.classes.get(&class_name))
                                .and_then(|class| class.methods.get(field))
                                .filter(|method| method.decl.receiver.is_none())
                                .map(|_| format!("{module_path}.{class_name}"))
                        },
                    ),
                };
                if let Some(owner) = associated_owner {
                    return Err(Diagnostic::coded_at(
                        "AU2005",
                        expr.span,
                        format!(
                            "associated method values are not supported in this language version; call `{owner}.{field}(...)` directly or wrap it in a named function"
                        ),
                    ));
                }
                if let Some(path) = self.member_access_path(expr) {
                    if let Some(binding) = locals.get(&path.root) {
                        if Self::field_path_is_moved(binding, &path.projections) {
                            let mut diagnostic = Diagnostic::at(
                                expr.span,
                                format!(
                                    "use of moved field `{}` from `{}`",
                                    path.projections, path.root
                                ),
                            );
                            if let Some(origin) =
                                Self::moved_field_origin(binding, &path.projections)
                            {
                                diagnostic = diagnostic
                                    .with_secondary(origin, "field moved here")
                                    .with_help(
                                        "use shared access to the field when ownership is not needed, or call `.clone()` before moving it when an independent value is required",
                                    );
                            }
                            return Err(diagnostic);
                        }
                    }
                    let through_view = locals
                        .get(&path.root)
                        .and_then(|binding| binding.view.as_ref())
                        .map(|_| path.root.as_str());
                    self.ensure_place_readable(&path, through_view, expr.span, locals)?;
                }
                if let Some((module_path, function_name)) = self.qualified_module_item(expr) {
                    if self
                        .module_namespace(&module_path)
                        .is_some_and(|namespace| {
                            namespace.extern_functions.contains_key(&function_name)
                        })
                    {
                        return Err(Diagnostic::coded_at(
                            "AU2999",
                            expr.span,
                            format!(
                                "extern function `{module_path}.{function_name}` is direct-call-only and cannot be used as a function value"
                            ),
                        )
                        .with_help(format!(
                            "call `{module_path}.{function_name}(...)` synchronously, or write a named Aura wrapper"
                        )));
                    }
                    if let Some(function) =
                        self.module_namespace(&module_path).and_then(|namespace| {
                            namespace
                                .functions
                                .get(&function_name)
                                .or_else(|| namespace.all_functions.get(&function_name))
                        })
                    {
                        return self.function_value_type(
                            function,
                            expected,
                            None,
                            expr.span,
                            &format!("function `{module_path}.{function_name}`"),
                        );
                    }
                }
                if let ExprKind::Specialize {
                    expr: inner,
                    type_args,
                } = &object.kind
                {
                    if let ExprKind::Name(enum_name) = &inner.kind {
                        let explicit_args = self.lower_explicit_type_args(type_args)?;
                        if let Ok(explicit_ty) =
                            self.explicit_builtin_type(enum_name, &explicit_args, expr.span)
                        {
                            if let Some(payload_tys) =
                                self.builtin_enum_variant_payload(&explicit_ty, enum_name, field)
                            {
                                if !payload_tys.is_empty() {
                                    return Err(Diagnostic::at(
                                        expr.span,
                                        format!(
                                            "variant `{}` of enum `{}` requires a payload",
                                            field, enum_name
                                        ),
                                    ));
                                }
                                return Ok(explicit_ty);
                            }
                        }
                        if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                            if explicit_args.len() != enum_info.decl.type_params.len() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "enum `{}` expects {} type argument{}, found {}",
                                        enum_name,
                                        enum_info.decl.type_params.len(),
                                        if enum_info.decl.type_params.len() == 1 {
                                            ""
                                        } else {
                                            "s"
                                        },
                                        explicit_args.len()
                                    ),
                                ));
                            }
                            let variant = enum_info.variants.get(field).ok_or_else(|| {
                                Diagnostic::at(
                                    expr.span,
                                    format!("enum `{}` has no variant `{}`", enum_name, field),
                                )
                            })?;
                            if !variant.payloads.is_empty() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "variant `{}` of enum `{}` requires a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            return Ok(Type::Named(
                                self.canonical_enum_info_name(enum_name, enum_info),
                                explicit_args,
                            ));
                        }
                    }
                }
                if let Some((module_path, enum_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(enum_info) = namespace.enums.get(&enum_name) {
                            let variant = enum_info.variants.get(field).ok_or_else(|| {
                                Diagnostic::at(
                                    expr.span,
                                    format!("enum `{}` has no variant `{}`", enum_name, field),
                                )
                            })?;
                            if !variant.payloads.is_empty() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "variant `{}` of enum `{}` requires a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            return Ok(Type::named(
                                self.module_enum_type_name(&module_path, enum_info),
                            ));
                        }
                    }
                }
                if let ExprKind::Name(enum_name) = &object.kind {
                    if expected.is_none() && enum_name == "Option" && field == "None" {
                        return Err(Diagnostic::at(
                            expr.span,
                            "cannot infer type parameter `T` for enum variant `Option.None`",
                        ));
                    }
                    if let Some(expected_ty) = expected {
                        if let Some(payload_tys) =
                            self.builtin_enum_variant_payload(expected_ty, enum_name, field)
                        {
                            if !payload_tys.is_empty() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "variant `{}` of enum `{}` requires a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            return Ok(expected_ty.clone());
                        }
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        let variant = enum_info.variants.get(field).ok_or_else(|| {
                            Diagnostic::at(
                                expr.span,
                                format!("enum `{}` has no variant `{}`", enum_name, field),
                            )
                        })?;
                        if !variant.payloads.is_empty() {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "variant `{}` of enum `{}` requires a payload",
                                    field, enum_name
                                ),
                            ));
                        }
                        if let Some(Type::Named(expected_name, expected_args)) = expected {
                            if self.canonical_enum_name(expected_name)
                                == self.canonical_enum_name(enum_name)
                            {
                                return Ok(Type::Named(
                                    self.canonical_enum_info_name(enum_name, enum_info),
                                    expected_args.clone(),
                                ));
                            }
                        }
                        if enum_info.decl.type_params.is_empty() {
                            return Ok(Type::named(
                                self.canonical_enum_info_name(enum_name, enum_info),
                            ));
                        }
                        let missing = enum_info
                            .decl
                            .type_params
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "T".to_string());
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "cannot infer type parameter `{}` for enum variant `{}.{}`",
                                missing, enum_name, field
                            ),
                        ));
                    }
                }
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                let member_ty = self.resolve_member_type(&object_ty, field, expr.span)?;
                Ok(member_ty)
            }
            ExprKind::Index { object, index } => {
                let function_target = match &object.kind {
                    ExprKind::Name(name) if !locals.contains_key(name) => self
                        .resolve_function_info(name)
                        .map(|function| (function, format!("function `{name}`"))),
                    ExprKind::Member { .. } => self.qualified_module_item(object).and_then(
                        |(module_path, function_name)| {
                            self.module_namespace(&module_path)
                                .and_then(|namespace| {
                                    namespace
                                        .functions
                                        .get(&function_name)
                                        .or(namespace.all_functions.get(&function_name))
                                })
                                .map(|function| {
                                    (
                                        function,
                                        format!("function `{module_path}.{function_name}`"),
                                    )
                                })
                        },
                    ),
                    _ => None,
                };
                if let Some((function, display_name)) = function_target {
                    let type_arg_exprs = match &index.kind {
                        ExprKind::Tuple(elements) => elements.as_slice(),
                        _ => std::slice::from_ref(&**index),
                    };
                    let type_refs = type_arg_exprs
                        .iter()
                        .map(Self::spawn_type_ref_from_expr)
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| {
                            Diagnostic::at(
                                index.span,
                                "function specialization expects type arguments",
                            )
                        })?;
                    let lowered = self.lower_explicit_type_args(&type_refs)?;
                    return self.function_value_type(
                        function,
                        expected,
                        Some(&lowered),
                        expr.span,
                        &display_name,
                    );
                }
                let object_ty = self.type_of_expr(object, locals)?;
                let locals_before_index = locals.clone();
                if let Type::Tuple(element_types) = &object_ty {
                    let tuple_index = match &index.kind {
                        ExprKind::Int(value) => usize::try_from(*value).ok(),
                        ExprKind::Group(inner) => match &inner.kind {
                            ExprKind::Int(value) => usize::try_from(*value).ok(),
                            _ => None,
                        },
                        _ => None,
                    }
                    .ok_or_else(|| {
                        Diagnostic::coded_at(
                            "AU2003",
                            index.span,
                            "tuple indices must be non-negative integer literals",
                        )
                    })?;
                    let element_ty = element_types.get(tuple_index).cloned().ok_or_else(|| {
                        Diagnostic::at(
                            index.span,
                            format!(
                                "tuple index {} is out of bounds for a {}-element tuple",
                                tuple_index,
                                element_types.len()
                            ),
                        )
                    })?;
                    if !self.is_copy_type(&element_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU3005",
                            expr.span,
                            format!(
                                "cannot consume non-copy tuple element `{}` by indexing; unpack the tuple to move its elements",
                                element_ty
                            ),
                        ));
                    }
                    return Ok(element_ty);
                }
                if let Some(element_ty) = array_element_type(&object_ty).cloned() {
                    self.check_array_index_type(index, locals)?;
                    let retained_base = self
                        .retained_place_access(
                            object,
                            &object_ty,
                            ReceiverKind::Borrow,
                            "index base",
                        )
                        .into_iter()
                        .collect::<Vec<_>>();
                    let mut index_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        index,
                        &locals_before_index,
                        &mut index_borrowed_places,
                    )?;
                    self.reject_retained_access_overlap(&retained_base, &index_borrowed_places)?;
                    return Ok(element_ty);
                }
                if let Some(element_ty) = vec_element_type(&object_ty).cloned() {
                    self.check_vec_index_type(index, index.span, locals)?;
                    let retained_base = self
                        .retained_place_access(
                            object,
                            &object_ty,
                            ReceiverKind::Borrow,
                            "index base",
                        )
                        .into_iter()
                        .collect::<Vec<_>>();
                    let mut index_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        index,
                        &locals_before_index,
                        &mut index_borrowed_places,
                    )?;
                    self.reject_retained_access_overlap(&retained_base, &index_borrowed_places)?;
                    let index_moved_places = self.newly_moved_places(&locals_before_index, locals);
                    self.reject_expr_borrow_move_overlap(
                        &retained_base,
                        &index_moved_places,
                        expr.span,
                    )?;
                    if !self.is_copy_type(&element_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU3005",
                            expr.span,
                            self.indexed_read_guidance("list", "index", &element_ty),
                        ));
                    }
                    return Ok(element_ty);
                }
                if let Some((key_ty, value_ty)) = map_key_value_types(&object_ty) {
                    self.require_array_equality_eligible(
                        key_ty,
                        format!("cannot use dict indexing with `{key_ty}`"),
                        index.span,
                    )?;
                    let index_ty = self.type_of_expr_hint(index, locals, Some(key_ty))?;
                    if index_ty != *key_ty {
                        return Err(Diagnostic::at(
                            index.span,
                            format!("map keys must have type `{}`, found `{}`", key_ty, index_ty),
                        ));
                    }
                    let retained_base = self
                        .retained_place_access(
                            object,
                            &object_ty,
                            ReceiverKind::Borrow,
                            "index base",
                        )
                        .into_iter()
                        .collect::<Vec<_>>();
                    let mut index_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        index,
                        &locals_before_index,
                        &mut index_borrowed_places,
                    )?;
                    self.reject_retained_access_overlap(&retained_base, &index_borrowed_places)?;
                    let index_moved_places = self.newly_moved_places(&locals_before_index, locals);
                    self.reject_expr_borrow_move_overlap(
                        &retained_base,
                        &index_moved_places,
                        expr.span,
                    )?;
                    if !self.is_copy_type(value_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU3005",
                            expr.span,
                            self.indexed_read_guidance("dict", "key", value_ty),
                        ));
                    }
                    return Ok(value_ty.clone());
                }
                Err(Diagnostic::at(
                    expr.span,
                    format!(
                        "cannot index non-Array, list, or dict value `{}`",
                        object_ty
                    ),
                ))
            }
            ExprKind::Slice {
                object,
                start,
                end,
                colon_span,
            } => {
                // Slicing observes the source rather than consuming it. Keep
                // that shared access live while each present endpoint is
                // checked in source order so endpoint calls cannot mutate or
                // move the retained place.
                let object_expected = expected.filter(|ty| {
                    array_element_type(ty).is_some()
                        || vec_element_type(ty).is_some()
                        || **ty == Type::named("str")
                });
                let object_ty = self.type_of_expr_hint(object, locals, object_expected)?;
                let vec_element = vec_element_type(&object_ty).cloned();
                let is_array = array_element_type(&object_ty).is_some();
                let is_string = object_ty == Type::named("str");
                if vec_element.is_none() && !is_array && !is_string {
                    return Err(Diagnostic::coded_at(
                        "AU2003",
                        expr.span,
                        format!(
                            "owned slicing is available only for `Array[T]`, `list[T]`, and `str`, found `{object_ty}`"
                        ),
                    )
                    .with_help(
                        "use indexing or a collection method supported by the base type, or convert the value to an Array, list, or str before slicing",
                    ));
                }

                let retained_base = self
                    .retained_place_access(object, &object_ty, ReceiverKind::Borrow, "slice base")
                    .into_iter()
                    .collect::<Vec<_>>();
                for endpoint in [start.as_deref(), end.as_deref()].into_iter().flatten() {
                    let locals_before_endpoint = locals.clone();
                    let endpoint_ty = self.check_slice_endpoint_type(endpoint, locals)?;
                    self.reject_retained_expr_overlap(
                        &retained_base,
                        endpoint,
                        &endpoint_ty,
                        None,
                        &locals_before_endpoint,
                        locals,
                        "slice endpoint",
                    )?;
                }

                if let Some(element_ty) = vec_element {
                    self.reject_rng_duplication("list slice", &element_ty, *colon_span)?;
                }
                Ok(object_ty)
            }
            ExprKind::Call { callee, args } => {
                self.type_of_call(callee, args, expr.span, locals, expected)
            }
        }
    }

    fn binary_uses_builtin_value_semantics(op: BinaryOp, left_ty: &Type, right_ty: &Type) -> bool {
        if array_element_type(left_ty).is_some() || array_element_type(right_ty).is_some() {
            return true;
        }
        if is_duration_type(left_ty) || is_duration_type(right_ty) {
            return true;
        }
        if matches!(
            op,
            BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Less
                | BinaryOp::LessEq
                | BinaryOp::Greater
                | BinaryOp::GreaterEq
        ) && (matches!(left_ty, Type::Tuple(_)) || matches!(right_ty, Type::Tuple(_)))
        {
            return true;
        }
        if left_ty != right_ty {
            return false;
        }
        match op {
            BinaryOp::And | BinaryOp::Or => *left_ty == Type::named("bool"),
            BinaryOp::Add => {
                is_integer_type(left_ty) || is_float_type(left_ty) || *left_ty == Type::named("str")
            }
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::FloorDiv | BinaryOp::Mod => {
                is_integer_type(left_ty) || is_float_type(left_ty)
            }
            BinaryOp::Pow => is_integer_type(left_ty) || is_float_type(left_ty),
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => is_integer_type(left_ty),
            BinaryOp::Eq | BinaryOp::NotEq => true,
            BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq => {
                is_integer_type(left_ty) || is_float_type(left_ty)
            }
        }
    }

    fn is_numeric_literal_expr(expr: &Expr) -> bool {
        Self::is_integer_literal_expr(expr) || matches!(expr.kind, ExprKind::Float(_))
    }

    /// The expected type a chain's left operand can adopt from its first link.
    fn chain_operand_hint(
        &self,
        left: &Expr,
        link: &CompareLink,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<Type> {
        match link.op.as_binary_op() {
            Some(BinaryOp::Eq | BinaryOp::NotEq) => {
                self.equality_operand_hint(left, &link.operand, locals)
            }
            Some(_) => None,
            None => self
                .type_of_expr_without_move_state(&link.operand, locals, None)
                .ok()
                .and_then(|operand_ty| membership_needle_type(&operand_ty)),
        }
    }

    fn check_membership_operands(
        &self,
        value_ty: &Type,
        container_ty: &Type,
        value_span: crate::diag::Span,
        operator_span: crate::diag::Span,
    ) -> Result<()> {
        let Some(needle_ty) = membership_needle_type(container_ty) else {
            return Err(Diagnostic::coded_at(
                "AU2003",
                operator_span,
                format!(
                    "`in` requires a `list[T]`, `set[T]`, `dict[K, V]`, or `str` container, found `{}`",
                    container_ty
                ),
            )
            .with_help(
                "membership tests read `list` and `set` elements, `dict` keys, and `str` substrings",
            ));
        };
        if *value_ty != needle_ty {
            let subject = match container_ty {
                Type::Named(name, _) if name == "dict" => "key",
                Type::Named(name, _) if name == "str" => "substring",
                _ => "element",
            };
            return Err(Diagnostic::coded_at(
                "AU2002",
                value_span,
                format!(
                    "`in` expects a `{}` {}, found `{}`",
                    needle_ty, subject, value_ty
                ),
            ));
        }
        self.require_array_equality_eligible(
            &needle_ty,
            format!("cannot test membership for `{needle_ty}`"),
            operator_span,
        )?;
        Ok(())
    }

    fn type_of_binary(
        &self,
        span: crate::diag::Span,
        op: BinaryOp,
        left_ty: Type,
        right_ty: Type,
    ) -> Result<Type> {
        let left_array_dtype = array_element_type(&left_ty);
        let right_array_dtype = array_element_type(&right_ty);
        if left_array_dtype.is_some() || right_array_dtype.is_some() {
            if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                return Err(Diagnostic::coded_at(
                    "AU2003",
                    span,
                    "Array equality is not supported",
                ));
            }
            if !matches!(
                op,
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
            ) {
                return Err(Diagnostic::coded_at(
                    "AU2003",
                    span,
                    format!(
                        "operator `{}` is not supported for Array values",
                        unsupported_array_operator_name(op)
                    ),
                ));
            }
            let dtype = match (left_array_dtype, right_array_dtype) {
                (Some(left_dtype), Some(right_dtype)) => {
                    if left_dtype != right_dtype {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            span,
                            format!(
                                "Array arithmetic requires matching dtypes, found `{left_ty}` and `{right_ty}`"
                            ),
                        ));
                    }
                    left_dtype
                }
                (Some(dtype), None) => {
                    if right_ty != *dtype {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            span,
                            format!(
                                "Array arithmetic requires scalar dtype `{dtype}`, found `{right_ty}`"
                            ),
                        ));
                    }
                    dtype
                }
                (None, Some(dtype)) => {
                    if left_ty != *dtype {
                        return Err(Diagnostic::coded_at(
                            "AU2002",
                            span,
                            format!(
                                "Array arithmetic requires scalar dtype `{dtype}`, found `{left_ty}`"
                            ),
                        ));
                    }
                    dtype
                }
                (None, None) => unreachable!("Array operand guard should retain one dtype"),
            };
            if op == BinaryOp::Div
                && matches!(
                    dtype,
                    Type::Named(name, args)
                        if args.is_empty() && matches!(name.as_str(), "int32" | "int64")
                )
            {
                return Err(Diagnostic::coded_at(
                    "AU2003",
                    span,
                    "integer Array `/` is not supported",
                )
                .with_help("map the values to `float32` or `float64` before using true division"));
            }
            return Ok(Type::Named("Array".to_string(), vec![dtype.clone()]));
        }
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq)
            && (matches!(left_ty, Type::Function { .. } | Type::Closure { .. })
                || matches!(right_ty, Type::Function { .. } | Type::Closure { .. }))
        {
            return Err(Diagnostic::coded_at(
                "AU2008",
                span,
                "callable equality is not supported; compare results or use an explicit discriminant",
            )
            .with_note(NO_IDENTITY_EQUALITY_NOTE));
        }
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            self.require_array_equality_eligible(
                &left_ty,
                format!("cannot compare `{left_ty}`"),
                span,
            )?;
            if right_ty != left_ty {
                self.require_array_equality_eligible(
                    &right_ty,
                    format!("cannot compare `{right_ty}`"),
                    span,
                )?;
            }
        }
        if let Some(result) = builtin_duration_binary_result(op, &left_ty, &right_ty) {
            return Ok(result);
        }
        let opaque_operand = if self.is_opaque_handle_type(&left_ty) {
            Some(&left_ty)
        } else if self.is_opaque_handle_type(&right_ty) {
            Some(&right_ty)
        } else {
            None
        };
        if let Some(handle_ty) = opaque_operand {
            let arithmetic_operator = match op {
                BinaryOp::Add => Some("+"),
                BinaryOp::Sub => Some("-"),
                BinaryOp::Mul => Some("*"),
                BinaryOp::Div => Some("/"),
                BinaryOp::FloorDiv => Some("//"),
                BinaryOp::Mod => Some("%"),
                _ => None,
            };
            if let Some(operator) = arithmetic_operator {
                return Err(Diagnostic::coded_at(
                    "AU2003",
                    span,
                    format!(
                        "opaque FFI handle `{handle_ty}` does not support operator `{operator}`; FFI v0 does not expose raw pointer arithmetic"
                    ),
                )
                .with_help(
                    "declare a reviewed extern function for the native handle operation instead of manipulating its address",
                ));
            }
            let ordering_operator = match op {
                BinaryOp::Less => Some("<"),
                BinaryOp::LessEq => Some("<="),
                BinaryOp::Greater => Some(">"),
                BinaryOp::GreaterEq => Some(">="),
                _ => None,
            };
            if let Some(operator) = ordering_operator {
                return Err(Diagnostic::coded_at(
                    "AU2003",
                    span,
                    format!(
                        "opaque FFI handle `{handle_ty}` does not support operator `{operator}`; FFI v0 does not define ordering for foreign addresses"
                    ),
                )
                .with_help(
                    "compare a stable scalar or str ordering key exposed by the binding instead of a foreign address",
                ));
            }
        }
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            let handle_comparison = if let Some(handle_ty) = self.opaque_handle_in_type(&left_ty) {
                Some((&left_ty, handle_ty))
            } else {
                self.opaque_handle_in_type(&right_ty)
                    .map(|handle_ty| (&right_ty, handle_ty))
            };
            if let Some((compared_ty, handle_ty)) = handle_comparison {
                return Err(Diagnostic::coded_at(
                    "AU2003",
                    span,
                    format!(
                        "cannot compare `{compared_ty}` because it contains opaque FFI handle `{handle_ty}` and FFI v0 does not define equality for foreign identity"
                    ),
                )
                .with_help(
                    "compare a stable scalar or str identifier exposed by the binding instead of a foreign address",
                ));
            }
        }
        if matches!(
            op,
            BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq
        ) && (matches!(left_ty, Type::Tuple(_)) || matches!(right_ty, Type::Tuple(_)))
        {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                "tuple ordering is not supported; use `==` or `!=`, or compare tuple elements explicitly",
            ));
        }
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq)
            && left_ty != right_ty
            && (matches!(left_ty, Type::Tuple(_)) || matches!(right_ty, Type::Tuple(_)))
        {
            return Err(Diagnostic::coded_at(
                "AU2002",
                span,
                format!(
                    "tuple equality operands must have the same type, found `{}` and `{}`",
                    left_ty, right_ty
                ),
            ));
        }
        if is_duration_type(&left_ty) || is_duration_type(&right_ty) {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                format!(
                    "unsupported Duration operands: `{}` and `{}`; supported forms are `Duration + Duration`, `Duration - Duration`, `Duration * int64`, `int64 * Duration`, `Duration // int64`, and comparisons between two Duration values",
                    left_ty, right_ty
                ),
            ));
        }
        if op == BinaryOp::Div && left_ty == right_ty && is_integer_type(&left_ty) {
            return Err(Diagnostic::at(
                span,
                "integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division",
            ));
        }
        if matches!(
            op,
            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr
        ) && (!is_integer_type(&left_ty) || !is_integer_type(&right_ty))
        {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                format!(
                    "bitwise and shift operators require integer operands, found `{left_ty}` and `{right_ty}`"
                ),
            ));
        }
        if op == BinaryOp::Pow && (!is_numeric_type(&left_ty) || !is_numeric_type(&right_ty)) {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                format!("power requires numeric operands, found `{left_ty}` and `{right_ty}`"),
            ));
        }
        if matches!(
            op,
            BinaryOp::Pow
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
        ) && left_ty != right_ty
        {
            return Err(Diagnostic::coded_at(
                "AU2002",
                span,
                format!(
                    "binary operator operands must match exactly, found `{left_ty}` and `{right_ty}`"
                ),
            ));
        }
        match (op, &left_ty, &right_ty) {
            (BinaryOp::And | BinaryOp::Or, Type::Named(name, args), _)
                if args.is_empty() && name == "bool" && left_ty == right_ty =>
            {
                Ok(Type::named("bool"))
            }
            (BinaryOp::Add, Type::Named(name, args), _)
                if args.is_empty()
                    && left_ty == right_ty
                    && (is_integer_type(&left_ty) || is_float_type(&left_ty) || name == "str") =>
            {
                Ok(left_ty)
            }
            (
                BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::FloorDiv | BinaryOp::Mod,
                _,
                _,
            ) if left_ty == right_ty && (is_integer_type(&left_ty) || is_float_type(&left_ty)) => {
                Ok(left_ty)
            }
            (BinaryOp::Pow, _, _)
                if left_ty == right_ty
                    && (is_integer_type(&left_ty) || is_float_type(&left_ty)) =>
            {
                Ok(left_ty)
            }
            (
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr,
                _,
                _,
            ) if left_ty == right_ty && is_integer_type(&left_ty) => Ok(left_ty),
            (BinaryOp::Eq | BinaryOp::NotEq, _, _) if left_ty == right_ty => {
                Ok(Type::named("bool"))
            }
            (BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq, _, _)
                if left_ty == right_ty
                    && (is_integer_type(&left_ty) || is_float_type(&left_ty)) =>
            {
                Ok(Type::named("bool"))
            }
            _ => {
                if let Some(operator) =
                    self.type_of_binary_operator_via_trait(span, op, &left_ty, &right_ty)?
                {
                    Ok(operator.return_type)
                } else if left_ty != right_ty {
                    let non_optional_none_type = if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                        match (&left_ty, &right_ty) {
                            (Type::Unit, other) if !is_option_type(other) => Some(other),
                            (other, Type::Unit) if !is_option_type(other) => Some(other),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(non_optional_ty) = non_optional_none_type {
                        return Err(Diagnostic::at(
                            span,
                            format!(
                                "type `{}` is not optional; only `Option[T]` values can be compared with `None`",
                                non_optional_ty
                            ),
                        ));
                    }
                    Err(Diagnostic::at(
                        span,
                        format!(
                            "binary operator operands must match, found `{}` and `{}`",
                            left_ty, right_ty
                        ),
                    ))
                } else {
                    Err(Diagnostic::at(
                        span,
                        format!("unsupported operands for binary expression: `{}`", left_ty),
                    ))
                }
            }
        }
    }

    fn type_of_unary_operator_via_trait(
        &self,
        span: crate::diag::Span,
        op: UnaryOp,
        value_ty: &Type,
    ) -> Result<Option<ResolvedUnaryOperatorAccess>> {
        let Some((trait_name, method_name)) = unary_operator_trait(op) else {
            return Ok(None);
        };
        if let Type::TypeParam(type_param_name) = value_ty {
            let Some(method) = self.operator_method_from_type_param(
                type_param_name,
                trait_name,
                method_name,
                None,
            )?
            else {
                return Ok(None);
            };
            let operation = format!("operator trait `{}.{}`", trait_name, method_name);
            self.enforce_resolved_rng_clone_obligations_before_method_inference(
                &operation,
                &method.rng_clone_safe_types,
                &method.decl.type_params,
                span,
            )?;
            self.enforce_resolved_array_equality_obligations_before_method_inference(
                &operation,
                &method.array_equality_safe_types,
                span,
            )?;
            let substitutions = self.infer_method_type_substitutions(
                &operation,
                &method.decl.type_params,
                &method.signature.params,
                &method.type_param_bounds,
                &method.signature.rng_clone_safe_type_params,
                &method.signature.array_equality_safe_type_params,
                &[],
                HashMap::new(),
                span,
            )?;
            let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
            let return_type = substitute_type(&method.signature.return_type, &substitutions);
            return Ok(Some(ResolvedUnaryOperatorAccess {
                return_type,
                receiver_passing,
            }));
        }
        let Some((method, substitutions)) =
            self.operator_method_for_concrete_type(span, value_ty, trait_name, method_name, None)?
        else {
            return Ok(None);
        };
        let operation = format!("operator trait `{}.{}`", trait_name, method_name);
        self.enforce_resolved_rng_clone_obligations_before_method_inference(
            &operation,
            &method.rng_clone_safe_types,
            &method.decl.type_params,
            span,
        )?;
        self.enforce_resolved_array_equality_obligations_before_method_inference(
            &operation,
            &method.array_equality_safe_types,
            span,
        )?;
        let substitutions = self.infer_method_type_substitutions(
            &operation,
            &method.decl.type_params,
            &method.signature.params,
            &method.type_param_bounds,
            &method.signature.rng_clone_safe_type_params,
            &method.signature.array_equality_safe_type_params,
            &[],
            substitutions,
            span,
        )?;
        let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
        let return_type = substitute_type(&method.signature.return_type, &substitutions);
        Ok(Some(ResolvedUnaryOperatorAccess {
            return_type,
            receiver_passing,
        }))
    }

    fn type_of_binary_operator_via_trait(
        &self,
        span: crate::diag::Span,
        op: BinaryOp,
        left_ty: &Type,
        right_ty: &Type,
    ) -> Result<Option<ResolvedBinaryOperatorAccess>> {
        let Some((trait_name, method_name)) = binary_operator_trait(op) else {
            return Ok(None);
        };
        if let Type::TypeParam(type_param_name) = left_ty {
            let Some(method) = self.operator_method_from_type_param(
                type_param_name,
                trait_name,
                method_name,
                Some(right_ty),
            )?
            else {
                return Ok(None);
            };
            let operation = format!("operator trait `{}.{}`", trait_name, method_name);
            self.enforce_resolved_rng_clone_obligations_before_method_inference(
                &operation,
                &method.rng_clone_safe_types,
                &method.decl.type_params,
                span,
            )?;
            self.enforce_resolved_array_equality_obligations_before_method_inference(
                &operation,
                &method.array_equality_safe_types,
                span,
            )?;
            let substitutions = self.infer_method_type_substitutions(
                &operation,
                &method.decl.type_params,
                &method.signature.params,
                &method.type_param_bounds,
                &method.signature.rng_clone_safe_type_params,
                &method.signature.array_equality_safe_type_params,
                std::slice::from_ref(right_ty),
                HashMap::new(),
                span,
            )?;
            let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
            let rhs_passing = method
                .signature
                .param_passings
                .first()
                .copied()
                .unwrap_or(ReceiverKind::Value);
            let return_type = substitute_type(&method.signature.return_type, &substitutions);
            return Ok(Some(ResolvedBinaryOperatorAccess {
                return_type,
                receiver_passing,
                rhs_passing,
            }));
        }
        let Some((method, substitutions)) = self.operator_method_for_concrete_type(
            span,
            left_ty,
            trait_name,
            method_name,
            Some(right_ty),
        )?
        else {
            return Ok(None);
        };
        let operation = format!("operator trait `{}.{}`", trait_name, method_name);
        self.enforce_resolved_rng_clone_obligations_before_method_inference(
            &operation,
            &method.rng_clone_safe_types,
            &method.decl.type_params,
            span,
        )?;
        self.enforce_resolved_array_equality_obligations_before_method_inference(
            &operation,
            &method.array_equality_safe_types,
            span,
        )?;
        let substitutions = self.infer_method_type_substitutions(
            &operation,
            &method.decl.type_params,
            &method.signature.params,
            &method.type_param_bounds,
            &method.signature.rng_clone_safe_type_params,
            &method.signature.array_equality_safe_type_params,
            std::slice::from_ref(right_ty),
            substitutions,
            span,
        )?;
        let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
        let rhs_passing = method
            .signature
            .param_passings
            .first()
            .copied()
            .unwrap_or(ReceiverKind::Value);
        let return_type = substitute_type(&method.signature.return_type, &substitutions);
        if matches!(
            op,
            BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq
        ) && return_type != Type::named("bool")
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "operator trait `{}` for `{}` must return `bool`",
                    trait_name, method_name
                ),
            ));
        }
        Ok(Some(ResolvedBinaryOperatorAccess {
            return_type,
            receiver_passing,
            rhs_passing,
        }))
    }

    fn type_check_builtin_class_constructor(
        &self,
        constructor: BuiltinClassConstructor,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        has_explicit_type_args: bool,
        result_type: Type,
    ) -> Result<Type> {
        if has_explicit_type_args {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`{}` does not take explicit type arguments",
                    constructor.qualified_name()
                ),
            ));
        }
        let ordered_args = constructor.bind_args(args, span)?;
        let seed_arg = required_ordered_arg(
            &ordered_args,
            0,
            span,
            "internal error: random.Rng should bind one seed argument",
        )?;
        let actual =
            self.type_of_expr_hint(&seed_arg.value, locals, Some(&Type::named("int64")))?;
        if actual != Type::named("int64") {
            return Err(Diagnostic::coded_at(
                "AU2002",
                seed_arg.span,
                format!(
                    "`{}` expects `int64` for `seed`, found `{}`",
                    constructor.qualified_name(),
                    actual
                ),
            ));
        }
        Ok(result_type)
    }

    #[allow(clippy::too_many_arguments)]
    fn type_check_user_class_constructor(
        &self,
        class: &ClassInfo,
        surface_name: &str,
        constructor_name: &str,
        class_type_name: String,
        explicit_type_args: Option<&[TypeRef]>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let mut substitutions = if let Some(type_args) = explicit_type_args {
            self.explicit_type_substitutions(
                &class.decl.type_params,
                type_args,
                span,
                &format!("class constructor `{}`", constructor_name),
            )?
        } else {
            match expected {
                Some(Type::Named(expected_name, expected_args))
                    if expected_name == &class_type_name
                        && expected_args.len() == class.decl.type_params.len() =>
                {
                    substitutions_from_decl_type_args(&class.decl.type_params, expected_args)
                }
                _ => HashMap::new(),
            }
        };
        let mut provided = HashMap::new();
        let mut next_positional_field = 0usize;
        let mut saw_named = false;
        for argument in args {
            let field_name = if let Some(field_name) = argument.name.as_ref() {
                saw_named = true;
                field_name
            } else {
                if saw_named {
                    return Err(Diagnostic::at(
                        argument.span,
                        "positional class constructor arguments must come before named arguments",
                    ));
                }
                let Some(field_decl) = class.decl.fields.get(next_positional_field) else {
                    return Err(Diagnostic::at(
                        argument.span,
                        format!(
                            "class constructor `{}` received too many positional arguments",
                            surface_name
                        ),
                    ));
                };
                next_positional_field += 1;
                &field_decl.name
            };
            let Some(field_info) = class.fields.get(field_name) else {
                return Err(Diagnostic::at(
                    argument.span,
                    format!(
                        "class `{}` has no field named `{}`",
                        surface_name, field_name
                    ),
                ));
            };
            if self.is_external_module(&class.module_name) && !field_info.public {
                return Err(Diagnostic::at(
                    argument.span,
                    format!("field `{}` is private on `{}`", field_name, class.decl.name),
                ));
            }
            if provided.insert(field_name.clone(), ()).is_some() {
                return Err(Diagnostic::at(
                    argument.span,
                    format!("field `{}` was provided more than once", field_name),
                ));
            }

            let hinted_field_ty = substitute_type(&field_info.ty, &substitutions);
            if self
                .direct_view_value_kind(&argument.value, locals)?
                .is_some()
            {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    argument.value.span,
                    format!("view cannot be stored in field `{field_name}` of `{surface_name}`"),
                )
                .with_help("store an explicit owned clone instead"));
            }
            let actual =
                match self.type_of_expr_hint(&argument.value, locals, Some(&hinted_field_ty)) {
                    Ok(actual) => actual,
                    Err(error)
                        if has_unresolved_type_params(&hinted_field_ty)
                            && !self.expr_can_use_partial_expected_hint(&argument.value) =>
                    {
                        match self.type_of_expr(&argument.value, locals) {
                            Ok(actual) => actual,
                            Err(_) => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
                };
            if let Err(error) = unify_type_pattern(&field_info.ty, &actual, &mut substitutions) {
                return Err(Diagnostic::at(
                    argument.span,
                    format!(
                        "field `{}` expects `{}`, found `{}` ({})",
                        field_name, hinted_field_ty, actual, error.message
                    ),
                ));
            }
            let resolved_field_ty = substitute_type(&field_info.ty, &substitutions);
            if type_contains_loan_closure(&actual) {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    argument.value.span,
                    "a closure containing a live view cannot be stored in a class field",
                )
                .with_help(
                    "keep the loan closure in its matching inferred local and call it directly",
                ));
            }
            if type_contains_closure_value(&actual) {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    argument.value.span,
                    format!("expected `{resolved_field_ty}`, found `{actual}`"),
                )
                .with_help(
                    "capturing closures cannot be stored in fields because a written `def` type erases capture metadata; use a named function or a capture-free lambda",
                ));
            }
            // Replay even Copy-valued constructor arguments. The replay is a
            // no-op for ordinary Copy values, but it recursively rejects a
            // returned-view result hidden behind a conditional or match arm.
            self.consume_value_expr(&argument.value, locals)?;
        }

        for field in &class.decl.fields {
            if !provided.contains_key(&field.name) && field.default.is_none() {
                if self.is_external_module(&class.module_name) && !field.public {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class constructor `{}` cannot initialize private field `{}` from another module",
                            class.decl.name, field.name
                        ),
                    ));
                }
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "class constructor `{}` is missing required field `{}`",
                        surface_name, field.name
                    ),
                ));
            }
        }

        let mut resolved_args = Vec::with_capacity(class.decl.type_params.len());
        for type_param in &class.decl.type_params {
            let Some(resolved) = substitutions.get(type_param).cloned() else {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for class constructor `{}`",
                        type_param, constructor_name
                    ),
                ));
            };
            resolved_args.push(resolved);
        }
        let resolved_substitutions =
            substitutions_from_decl_type_args(&class.decl.type_params, &resolved_args);
        for (index, type_param) in class.decl.type_params.iter().enumerate() {
            let Some(bounds) = class.type_param_bounds.get(type_param) else {
                continue;
            };
            let mut resolved_bounds = Vec::with_capacity(bounds.len());
            for bound in bounds {
                resolved_bounds.push(substitute_trait_bound(bound, &resolved_substitutions));
            }
            self.assert_type_satisfies_bounds(&resolved_args[index], &resolved_bounds, span)?;
        }

        Ok(Type::Named(class_type_name, resolved_args))
    }

    #[allow(clippy::too_many_arguments)]
    fn type_check_user_enum_variant_constructor(
        &self,
        enum_info: &EnumInfo,
        enum_name: &str,
        enum_type_name: String,
        variant_name: &str,
        explicit_type_args: Option<&[TypeRef]>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let Some(variant) = enum_info.variants.get(variant_name) else {
            return Err(Diagnostic::at(
                span,
                format!("enum `{}` has no variant `{}`", enum_name, variant_name),
            ));
        };
        let mut substitutions = if let Some(type_args) = explicit_type_args {
            self.explicit_type_substitutions(
                &enum_info.decl.type_params,
                type_args,
                span,
                &format!("enum `{}`", enum_name),
            )?
        } else {
            match expected {
                Some(Type::Named(expected_name, expected_args))
                    if expected_name == &enum_type_name
                        && expected_args.len() == enum_info.decl.type_params.len() =>
                {
                    substitutions_from_decl_type_args(&enum_info.decl.type_params, expected_args)
                }
                _ => HashMap::new(),
            }
        };
        let ordered_args = self.variant_payload_arguments(
            args,
            span,
            variant_name,
            enum_name,
            &variant.payloads,
            variant.named_payloads,
        )?;
        for (argument, payload) in ordered_args.iter().zip(variant.payloads.iter()) {
            self.reject_owned_view_value(&argument.value, locals, "an enum payload")?;
            let hinted_payload_ty = substitute_type(&payload.ty, &substitutions);
            let actual = if has_unresolved_type_params(&hinted_payload_ty) {
                self.type_of_expr(&argument.value, locals)?
            } else {
                self.type_of_expr_hint(&argument.value, locals, Some(&hinted_payload_ty))?
            };
            if let Err(error) = unify_type_pattern(&payload.ty, &actual, &mut substitutions) {
                return Err(Diagnostic::at(
                    argument.span,
                    format!(
                        "variant `{}` of enum `{}` expects `{}`, found `{}` ({})",
                        variant_name, enum_name, hinted_payload_ty, actual, error.message
                    ),
                ));
            }
            if type_contains_loan_closure(&actual) {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    argument.value.span,
                    "a closure containing a live view cannot be stored in an enum payload",
                )
                .with_help(
                    "keep the loan closure in its matching inferred local and call it directly",
                ));
            }
            // Replaying Copy payloads is normally a no-op, but a view of a
            // Copy pointee is still a descriptor and must not decay into an
            // owned enum snapshot.
            self.consume_value_expr(&argument.value, locals)?;
        }

        let mut resolved_args = Vec::with_capacity(enum_info.decl.type_params.len());
        for type_param in &enum_info.decl.type_params {
            let Some(resolved) = substitutions.get(type_param).cloned() else {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for enum variant `{}.{}`",
                        type_param, enum_name, variant_name
                    ),
                ));
            };
            resolved_args.push(resolved);
        }
        let resolved_substitutions =
            substitutions_from_decl_type_args(&enum_info.decl.type_params, &resolved_args);
        for (index, type_param) in enum_info.decl.type_params.iter().enumerate() {
            let Some(bounds) = enum_info.type_param_bounds.get(type_param) else {
                continue;
            };
            let mut resolved_bounds = Vec::with_capacity(bounds.len());
            for bound in bounds {
                resolved_bounds.push(substitute_trait_bound(bound, &resolved_substitutions));
            }
            self.assert_type_satisfies_bounds(&resolved_args[index], &resolved_bounds, span)?;
        }

        Ok(Type::Named(enum_type_name, resolved_args))
    }

    fn type_of_call(
        &self,
        callee: &Expr,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        // The parser preserves `f[T](...)` as `Specialize`, while a grouping
        // boundary necessarily leaves `(f[T])(...)` as `Group(Index(...))`.
        // Recover the same direct-call specialization only when the indexed
        // object is known to be a returned-view callable; ordinary runtime
        // indexing remains untouched.
        let grouped = grouped_expr(callee);
        let mut grouped_specialization_base = None;
        let mut grouped_specialization_args = None;
        if matches!(&callee.kind, ExprKind::Group(_)) {
            if let ExprKind::Index { object, index } = &grouped.kind {
                if self
                    .returned_view_callee(object, locals)?
                    .is_some_and(|(decl, _, _)| decl.view_return.is_some())
                {
                    let type_arg_exprs = match &index.kind {
                        ExprKind::Tuple(elements) => elements.as_slice(),
                        _ => std::slice::from_ref(&**index),
                    };
                    let type_refs = type_arg_exprs
                        .iter()
                        .map(Self::spawn_type_ref_from_expr)
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| {
                            Diagnostic::at(
                                index.span,
                                "function specialization expects type arguments",
                            )
                        })?;
                    grouped_specialization_base = Some(object.as_ref());
                    grouped_specialization_args = Some(type_refs);
                }
            }
        }
        let (base_callee, explicit_type_args) = match (
            grouped_specialization_base,
            grouped_specialization_args.as_deref(),
        ) {
            (Some(base), Some(type_args)) => (base, Some(type_args)),
            _ => self.peel_specialization(callee),
        };

        let resolve_alias = |expr: &Expr| match &expr.kind {
            ExprKind::Name(name) if !locals.contains_key(name) => self
                .current_module_namespace()
                .and_then(|namespace| namespace.all_aliases.get(name))
                .or_else(|| self.type_names.checked_aliases.get(name))
                .or_else(|| self.type_names.imported_aliases.get(name)),
            ExprKind::Member { .. } => {
                self.qualified_module_item(expr).and_then(|(module, name)| {
                    self.module_namespace(&module)
                        .and_then(|namespace| namespace.aliases.get(&name))
                })
            }
            _ => None,
        };
        let inferred_alias = resolve_alias(grouped_expr(callee));
        if let Some(expanded) = expand_alias_callee(
            callee,
            &resolve_alias,
            &|ty| {
                lower_type(
                    ty,
                    self.type_names,
                    self.type_arities,
                    self.canonical_type_names,
                    &self.type_params,
                )
            },
            &|alias, types, span| {
                self.check_alias_constructor_bounds(
                    alias,
                    &substitutions_from_decl_type_args(&alias.decl.type_params, types),
                    span,
                )
            },
            self.module_name,
            self.canonical_type_names,
            self.type_names.expansion_budget(),
        )? {
            let result = self.type_of_call(&expanded, args, span, locals, expected)?;
            if let Some(alias) = inferred_alias {
                let mut substitutions = HashMap::new();
                unify_type_pattern(&alias.target, &result, &mut substitutions).map_err(
                    |error| {
                        Diagnostic::coded_at(
                            "AU2002",
                            span,
                            format!(
                                "constructor for alias `{}`: {}",
                                alias.decl.name, error.message
                            ),
                        )
                    },
                )?;
                self.check_alias_constructor_bounds(alias, &substitutions, span)?;
            }
            return Ok(result);
        }

        let extern_target = match &base_callee.kind {
            ExprKind::Name(name) if !locals.contains_key(name) => self
                .resolve_extern_function_info(name)
                .map(|function| (function, format!("extern function `{name}`"))),
            ExprKind::Member { .. } => {
                self.qualified_module_item(base_callee)
                    .and_then(|(module_path, function_name)| {
                        self.module_namespace(&module_path).and_then(|namespace| {
                            namespace
                                .extern_functions
                                .get(&function_name)
                                .map(|function| {
                                    (
                                        function,
                                        format!("extern function `{module_path}.{function_name}`"),
                                    )
                                })
                        })
                    })
            }
            _ => None,
        };
        if let Some((function, display_name)) = extern_target {
            if explicit_type_args.is_some() {
                return Err(Diagnostic::coded_at(
                    "AU2005",
                    span,
                    format!("{display_name} does not take type arguments"),
                ));
            }
            return self.type_check_callable_args(
                &display_name,
                &[],
                &function.decl.params,
                &function.signature.param_passings,
                &function.signature.params,
                &function.signature.return_type,
                &BTreeMap::new(),
                &BTreeSet::new(),
                &BTreeSet::new(),
                args,
                span,
                locals,
                expected,
                HashMap::new(),
            );
        }

        if let ExprKind::Name(name) = &base_callee.kind {
            if self.resolve_opaque_handle_info(name).is_some() {
                return Err(Diagnostic::coded_at(
                    "AU2005",
                    span,
                    format!(
                        "opaque FFI handle `{name}` cannot be constructed by Aura code"
                    ),
                )
                .with_help(
                    "opaque handles are returned by an extern function and have no Aura-visible layout or constructor",
                ));
            }
        }

        if matches!(&base_callee.kind, ExprKind::Name(name) if name == "TaskGroup")
            && explicit_type_args.is_none()
        {
            if !args.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    "`TaskGroup` does not take constructor arguments",
                ));
            }
            return Ok(Type::named("TaskGroup"));
        }

        if let (ExprKind::Name(name), Some(type_args)) = (&base_callee.kind, explicit_type_args) {
            if name == "Queue" {
                let explicit_args = self.lower_explicit_type_args(type_args)?;
                if explicit_args.len() != 1 {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` expects exactly one type argument, found {}",
                            name,
                            explicit_args.len()
                        ),
                    ));
                }
                self.require_queue_payload_transfer(&explicit_args[0], span)?;
                let capacity_params = [crate::call::CallableParam::optional("capacity")];
                let ordered_args = bind_call_arguments(
                    &format!("class `{}`", name),
                    &capacity_params,
                    args,
                    span,
                    CallConvention::PositionalOrNamed,
                )?;
                if let Some(capacity_arg) = ordered_args[0] {
                    let actual = self.type_of_expr_hint(
                        &capacity_arg.value,
                        locals,
                        Some(&Type::named("int32")),
                    )?;
                    if actual != Type::named("int32") {
                        return Err(Diagnostic::at(
                            capacity_arg.span,
                            format!("field `capacity` expects `int32`, found `{}`", actual),
                        ));
                    }
                }
                return Ok(Type::Named("Queue".to_string(), explicit_args));
            }
            if name == "list" {
                let explicit_args = self.lower_explicit_type_args(type_args)?;
                if explicit_args.len() != 1 {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` expects exactly one type argument, found {}",
                            name,
                            explicit_args.len()
                        ),
                    ));
                }
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "class `list` does not take constructor arguments; use a list literal or `append(...)`",
                    ));
                }
                return Ok(Type::Named("list".to_string(), explicit_args));
            }
            if name == "set" {
                let explicit_args = self.lower_explicit_type_args(type_args)?;
                if explicit_args.len() != 1 {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` expects exactly one type argument, found {}",
                            name,
                            explicit_args.len()
                        ),
                    ));
                }
                self.require_array_equality_eligible(
                    &explicit_args[0],
                    format!("cannot use `{}` as a set element", explicit_args[0]),
                    span,
                )?;
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "class `set` does not take constructor arguments; use a set literal or `add(...)`",
                    ));
                }
                return Ok(Type::Named("set".to_string(), explicit_args));
            }
            if name == "dict" {
                let explicit_args = self.lower_explicit_type_args(type_args)?;
                if explicit_args.len() != 2 {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` expects exactly two type arguments, found {}",
                            name,
                            explicit_args.len()
                        ),
                    ));
                }
                self.require_array_equality_eligible(
                    &explicit_args[0],
                    format!("cannot use `{}` as a dict key", explicit_args[0]),
                    span,
                )?;
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "class `dict` does not take constructor arguments; use a dict literal or indexed assignment",
                    ));
                }
                return Ok(Type::Named("dict".to_string(), explicit_args));
            }
            if name == "TaskGroup" {
                if !type_args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "`TaskGroup` does not take type arguments",
                    ));
                }
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "`TaskGroup` does not take constructor arguments",
                    ));
                }
                return Ok(Type::named("TaskGroup"));
            }
        }

        if let ExprKind::Member { object, field } = &base_callee.kind {
            if let ExprKind::Name(type_name) = &object.kind {
                if !locals.contains_key(type_name) {
                    if let Some(constructor) = BuiltinAssociatedFunction::resolve(type_name, field)
                    {
                        if explicit_type_args.is_some() {
                            return Err(Diagnostic::at(
                                span,
                                format!(
                                    "`{}.{}` does not take explicit type arguments",
                                    constructor.owner_name(),
                                    constructor.name()
                                ),
                            ));
                        }
                        let ordered_args = constructor.bind_args(args, span)?;
                        self.reject_builtin_associated_argument_sibling_overlap(
                            constructor,
                            args,
                            &ordered_args,
                            locals,
                        )?;
                        match constructor {
                            BuiltinAssociatedFunction::DurationMilliseconds
                            | BuiltinAssociatedFunction::DurationSeconds
                            | BuiltinAssociatedFunction::DurationMinutes => {
                                let value_arg = required_ordered_arg(
                                &ordered_args,
                                0,
                                span,
                                "internal error: Duration constructor should bind one value argument",
                            )?;
                                let actual = self.type_of_expr_hint(
                                    &value_arg.value,
                                    locals,
                                    Some(&Type::named("int64")),
                                )?;
                                if actual != Type::named("int64") {
                                    return Err(Diagnostic::at(
                                        value_arg.span,
                                        format!(
                                            "`Duration.{}` expects `int64`, found `{}`",
                                            constructor.name(),
                                            actual
                                        ),
                                    ));
                                }
                                return Ok(Type::named("Duration"));
                            }
                            BuiltinAssociatedFunction::StringFromBytes => {
                                let bytes_arg = required_ordered_arg(
                                    &ordered_args,
                                    0,
                                    span,
                                    "internal error: str.from_bytes should bind one bytes argument",
                                )?;
                                let expected =
                                    Type::Named("list".to_string(), vec![Type::named("uint8")]);
                                let actual = self.type_of_expr_hint(
                                    &bytes_arg.value,
                                    locals,
                                    Some(&expected),
                                )?;
                                if actual != expected {
                                    return Err(Diagnostic::at(
                                        bytes_arg.span,
                                        format!(
                                        "`str.from_bytes` expects `list[uint8]`, found `{actual}`"
                                    ),
                                    ));
                                }
                                return Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::named("str"), Type::named("bytes.Error")],
                                ));
                            }
                            BuiltinAssociatedFunction::ArrayZeros
                            | BuiltinAssociatedFunction::ArrayFull
                            | BuiltinAssociatedFunction::ArrayFromVec => {
                                return Err(Diagnostic::coded_at(
                                    "AU2005",
                                    object.span,
                                    "Array associated functions require an explicit dtype such as `Array[int32]`",
                                ));
                            }
                            BuiltinAssociatedFunction::ListWithCapacity
                            | BuiltinAssociatedFunction::DictWithCapacity
                            | BuiltinAssociatedFunction::SetWithCapacity => {
                                return Err(Diagnostic::coded_at(
                                    "AU2005",
                                    object.span,
                                    format!(
                                        "`{type_name}.with_capacity` requires explicit type arguments"
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }

        match &base_callee.kind {
            ExprKind::Name(name) if BuiltinFunction::from_name(name).is_some() => {
                let Some(builtin) = BuiltinFunction::from_name(name) else {
                    unreachable!("builtin lookup is stable during call checking");
                };
                let ordered_args = builtin.bind_args(args, span)?;
                self.reject_builtin_function_argument_sibling_overlap(
                    builtin,
                    args,
                    &ordered_args,
                    locals,
                )?;
                match builtin {
                    BuiltinFunction::Print => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `print` should bind exactly one argument",
                        )?;
                        self.type_of_expr(&value_arg.value, locals)?;
                        Ok(Type::Unit)
                    }
                    BuiltinFunction::Range => {
                        for (index, argument) in ordered_args.into_iter().enumerate() {
                            let Some(argument) = argument else {
                                continue;
                            };
                            self.check_index_domain_type(
                                &argument.value,
                                argument.span,
                                "`range` arguments",
                                locals,
                            )?;
                            self.apply_builtin_function_argument_passing(
                                builtin, index, argument, locals,
                            )?;
                        }
                        Ok(Type::named("Range"))
                    }
                    BuiltinFunction::Cancelled => Ok(Type::named("bool")),
                    BuiltinFunction::YieldNow => Ok(Type::Unit),
                    BuiltinFunction::Sleep => {
                        let duration_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `sleep` should bind exactly one argument",
                        )?;
                        let duration_ty = self.type_of_expr(&duration_arg.value, locals)?;
                        if duration_ty != Type::named("Duration") {
                            return Err(Diagnostic::at(
                                duration_arg.span,
                                format!(
                                    "`sleep(...)` expects a `Duration`, found `{}`",
                                    duration_ty
                                ),
                            ));
                        }
                        Ok(Type::Unit)
                    }
                    BuiltinFunction::Select => {
                        let mut queue_payload: Option<Type> = None;
                        let mut task_result: Option<Type> = None;
                        let mut nonrepeatable_tasks = Vec::new();

                        for (index, argument) in args.iter().enumerate() {
                            let source_ty = self.type_of_expr(&argument.value, locals)?;
                            match &source_ty {
                                Type::Named(name, source_args)
                                    if name == "Queue" && source_args.len() == 1 =>
                                {
                                    if let Some(expected) = queue_payload.as_ref() {
                                        if expected != &source_args[0] {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                argument.span,
                                                format!(
                                                    "all Queue sources in one `select` call must have the same payload type `{expected}`, found `Queue[{}]` at source {index}",
                                                    source_args[0]
                                                ),
                                            )
                                            .with_help(
                                                "wrap heterogeneous queue payloads in one explicit enum before selecting",
                                            ));
                                        }
                                    } else {
                                        queue_payload = Some(source_args[0].clone());
                                    }
                                }
                                Type::Named(name, source_args)
                                    if name == "Task" && source_args.len() == 1 =>
                                {
                                    if let Some(expected) = task_result.as_ref() {
                                        if expected != &source_args[0] {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                argument.span,
                                                format!(
                                                    "all Task sources in one `select` call must have the same result type `{expected}`, found `Task[{}]` at source {index}",
                                                    source_args[0]
                                                ),
                                            )
                                            .with_help(
                                                "wrap heterogeneous task results in one explicit enum before selecting",
                                            ));
                                        }
                                    } else {
                                        task_result = Some(source_args[0].clone());
                                    }
                                    self.reject_rng_duplication(
                                        "select",
                                        &source_args[0],
                                        argument.span,
                                    )?;
                                    if !self.is_copy_type(&source_ty) {
                                        nonrepeatable_tasks.push((
                                            argument,
                                            source_args[0].clone(),
                                            self.borrow_call_place(&argument.value),
                                        ));
                                    }
                                }
                                Type::Named(name, source_args)
                                    if name == "Duration" && source_args.is_empty() => {}
                                _ => {
                                    return Err(Diagnostic::coded_at(
                                        "AU2002",
                                        argument.span,
                                        format!(
                                            "`select` sources must be `Queue[Q]`, `Task[T]`, or `Duration`; found `{source_ty}` at source {index}"
                                        ),
                                    )
                                    .with_help(
                                        "pass one or more queue handles, task handles, or relative Duration values as positional sources",
                                    ));
                                }
                            }
                        }

                        for current in 0..nonrepeatable_tasks.len() {
                            let Some(current_place) = nonrepeatable_tasks[current].2.as_ref()
                            else {
                                continue;
                            };
                            if nonrepeatable_tasks[..current]
                                .iter()
                                .filter_map(|(_, _, place)| place.as_ref())
                                .any(|prior_place| prior_place == current_place)
                            {
                                return Err(Diagnostic::coded_at(
                                    "AU3009",
                                    nonrepeatable_tasks[current].0.span,
                                    format!(
                                        "one `select` call cannot use the same non-repeatable Task source `{}` more than once",
                                        self.render_place_expr(
                                            &nonrepeatable_tasks[current].0.value
                                        )
                                    ),
                                )
                                .with_help(
                                    "`select` consumes every non-repeatable Task source at call entry and abandons losing observation rights; pass each unique handle once",
                                ));
                            }
                        }

                        for (argument, result_ty, _) in nonrepeatable_tasks {
                            if let Err(mut diagnostic) = self.consume_task_observation_right(
                                &argument.value,
                                &result_ty,
                                "select",
                                locals,
                            ) {
                                if diagnostic.code == "AU3002" {
                                    diagnostic.message = format!(
                                        "`select` consumes every non-repeatable Task source at call entry, but `{}` is available only through shared access",
                                        self.render_place_expr(&argument.value)
                                    );
                                    diagnostic.help = vec![
                                        "pass the Task through owned access; losing observation rights are deliberately abandoned and cannot be cloned"
                                            .to_string(),
                                    ];
                                }
                                return Err(diagnostic);
                            }
                        }

                        Ok(Type::Named(
                            "SelectOutcome".to_string(),
                            vec![
                                queue_payload.unwrap_or(Type::Unit),
                                task_result.unwrap_or(Type::Unit),
                            ],
                        ))
                    }
                    BuiltinFunction::WaitAny | BuiltinFunction::WaitAll => {
                        let tasks_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            format!(
                                "internal error: `{}` should bind the `tasks` argument",
                                builtin.name()
                            ),
                        )?;
                        let tasks_ty = self.type_of_expr(&tasks_arg.value, locals)?;
                        let Type::Named(ref container_name, ref container_args) = tasks_ty else {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `list[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        };
                        if container_name != "list" || container_args.len() != 1 {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `list[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        }
                        let Type::Named(task_name, task_args) = &container_args[0] else {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `list[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        };
                        if task_name != "Task" || task_args.len() != 1 {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `list[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        }
                        self.reject_rng_duplication(builtin.name(), &task_args[0], span)?;
                        let task_handle_ty =
                            Type::Named("Task".to_string(), vec![task_args[0].clone()]);
                        if !self.is_copy_type(&task_handle_ty) {
                            self.reject_conditional_value_argument_overlap(
                                &tasks_arg.value,
                                &tasks_ty,
                                &format!("`{}` tasks argument", builtin.name()),
                                args,
                                Some(tasks_arg),
                                locals,
                            )?;
                        }
                        if let Some(timeout_arg) = ordered_args[1] {
                            let actual = self.type_of_expr_hint(
                                &timeout_arg.value,
                                locals,
                                Some(&Type::named("Duration")),
                            )?;
                            if actual != Type::named("Duration") {
                                return Err(Diagnostic::at(
                                    timeout_arg.span,
                                    format!(
                                        "`{}(timeout=...)` expects `Duration`, found `{}`",
                                        builtin.name(),
                                        actual
                                    ),
                                ));
                            }
                        }
                        self.consume_task_collection_observation_right(
                            &tasks_arg.value,
                            &task_args[0],
                            builtin.name(),
                            locals,
                        )?;
                        Ok(Type::Named(
                            match builtin {
                                BuiltinFunction::WaitAny => "WaitAny".to_string(),
                                BuiltinFunction::WaitAll => "WaitAll".to_string(),
                                _ => unreachable!(),
                            },
                            vec![task_args[0].clone()],
                        ))
                    }
                    BuiltinFunction::Len => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `len` should bind exactly one argument",
                        )?;
                        let value_ty = self.type_of_expr(&value_arg.value, locals)?;
                        let Type::Named(receiver_name, _) = &value_ty else {
                            return Err(Diagnostic::coded_at(
                                "AU2002",
                                value_arg.span,
                                format!("`len(...)` expects a value with a `len()` member, found `{value_ty}`"),
                            ));
                        };
                        if BuiltinMember::resolve(receiver_name, "len").is_none() {
                            return Err(Diagnostic::coded_at(
                                "AU2002",
                                value_arg.span,
                                format!("`len(...)` expects a value with a `len()` member, found `{value_ty}`"),
                            )
                            .with_help(
                                "`len` delegates to the value's own `len()`; `str`, `list[T]`, `dict[K, V]`, and `set[T]` provide it",
                            ));
                        }
                        Ok(Type::named("int64"))
                    }
                    BuiltinFunction::Str => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `str` should bind exactly one argument",
                        )?;
                        self.type_of_expr(&value_arg.value, locals)?;
                        Ok(Type::named("str"))
                    }
                    BuiltinFunction::Abs => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `abs` should bind exactly one argument",
                        )?;
                        let value_ty = self.type_of_expr(&value_arg.value, locals)?;
                        if !is_numeric_type(&value_ty) {
                            return Err(Diagnostic::at(
                                value_arg.span,
                                format!(
                                    "`abs(...)` expects an integer or float value, found `{}`",
                                    value_ty
                                ),
                            ));
                        }
                        Ok(value_ty)
                    }
                    BuiltinFunction::Min | BuiltinFunction::Max => {
                        let left_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            format!(
                                "internal error: `{}` should bind a left argument",
                                builtin.name()
                            ),
                        )?;
                        let left_ty = self.type_of_expr(&left_arg.value, locals)?;
                        if !is_numeric_type(&left_ty) {
                            return Err(Diagnostic::at(
                                left_arg.span,
                                format!(
                                    "`{}` expects numeric arguments, found `{}`",
                                    builtin.name(),
                                    left_ty
                                ),
                            ));
                        }
                        self.apply_builtin_function_argument_passing(builtin, 0, left_arg, locals)?;
                        let right_arg = required_ordered_arg(
                            &ordered_args,
                            1,
                            span,
                            format!(
                                "internal error: `{}` should bind a right argument",
                                builtin.name()
                            ),
                        )?;
                        let right_ty =
                            self.type_of_expr_hint(&right_arg.value, locals, Some(&left_ty))?;
                        if right_ty != left_ty {
                            return Err(Diagnostic::at(
                                right_arg.span,
                                format!(
                                    "`{}` arguments must match, found `{}` and `{}`",
                                    builtin.name(),
                                    left_ty,
                                    right_ty
                                ),
                            ));
                        }
                        self.apply_builtin_function_argument_passing(
                            builtin, 1, right_arg, locals,
                        )?;
                        Ok(left_ty)
                    }
                    BuiltinFunction::Sqrt => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `sqrt` should bind exactly one argument",
                        )?;
                        let value_ty = self.type_of_expr(&value_arg.value, locals)?;
                        if !matches!(
                            value_ty,
                            Type::Named(ref name, ref args)
                                if args.is_empty()
                                    && matches!(name.as_str(), "float32" | "float64")
                        ) {
                            return Err(Diagnostic::at(
                                value_arg.span,
                                format!(
                                    "`sqrt(...)` expects `float32` or `float64`, found `{}`",
                                    value_ty
                                ),
                            ));
                        }
                        Ok(value_ty)
                    }
                    BuiltinFunction::Round => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `round` should bind exactly one argument",
                        )?;
                        let value_ty = self.type_of_expr(&value_arg.value, locals)?;
                        if is_integer_type(&value_ty) {
                            Ok(value_ty)
                        } else if matches!(
                            value_ty,
                            Type::Named(ref name, ref args)
                                if args.is_empty()
                                    && matches!(name.as_str(), "float32" | "float64")
                        ) {
                            Ok(Type::named("int64"))
                        } else {
                            Err(Diagnostic::coded_at(
                                "AU2003",
                                value_arg.span,
                                format!(
                                    "`round(...)` expects an integer, `float32`, or `float64`, found `{value_ty}`"
                                ),
                            ))
                        }
                    }
                    BuiltinFunction::Divmod => {
                        let left_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `divmod` should bind a left argument",
                        )?;
                        let left_ty = self.type_of_expr(&left_arg.value, locals)?;
                        if !is_numeric_type(&left_ty) {
                            return Err(Diagnostic::coded_at(
                                "AU2003",
                                left_arg.span,
                                format!(
                                    "`divmod(...)` expects numeric arguments, found `{left_ty}`"
                                ),
                            ));
                        }
                        let right_arg = required_ordered_arg(
                            &ordered_args,
                            1,
                            span,
                            "internal error: `divmod` should bind a right argument",
                        )?;
                        let right_ty =
                            self.type_of_expr_hint(&right_arg.value, locals, Some(&left_ty))?;
                        if right_ty != left_ty {
                            return Err(Diagnostic::coded_at(
                                "AU2002",
                                right_arg.span,
                                format!(
                                    "`divmod(...)` arguments must have one exact type, found `{left_ty}` and `{right_ty}`"
                                ),
                            ));
                        }
                        Ok(Type::Tuple(vec![left_ty.clone(), left_ty]))
                    }
                    BuiltinFunction::ParseInt32 => {
                        let text_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `parse_int32` should bind exactly one argument",
                        )?;
                        let text_ty = self.type_of_expr_hint(
                            &text_arg.value,
                            locals,
                            Some(&Type::named("str")),
                        )?;
                        if text_ty != Type::named("str") {
                            return Err(Diagnostic::at(
                                text_arg.span,
                                format!("`parse_int32(...)` expects `str`, found `{}`", text_ty),
                            ));
                        }
                        Ok(Type::Named(
                            "Result".to_string(),
                            vec![Type::named("int32"), Type::named("str")],
                        ))
                    }
                    BuiltinFunction::ParseInt64 => {
                        let text_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `parse_int64` should bind exactly one argument",
                        )?;
                        let text_ty = self.type_of_expr_hint(
                            &text_arg.value,
                            locals,
                            Some(&Type::named("str")),
                        )?;
                        if text_ty != Type::named("str") {
                            return Err(Diagnostic::at(
                                text_arg.span,
                                format!("`parse_int64(...)` expects `str`, found `{}`", text_ty),
                            ));
                        }
                        Ok(Type::Named(
                            "Result".to_string(),
                            vec![Type::named("int64"), Type::named("str")],
                        ))
                    }
                    BuiltinFunction::ParseFloat64 => {
                        let text_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `parse_float64` should bind exactly one argument",
                        )?;
                        let text_ty = self.type_of_expr_hint(
                            &text_arg.value,
                            locals,
                            Some(&Type::named("str")),
                        )?;
                        if text_ty != Type::named("str") {
                            return Err(Diagnostic::at(
                                text_arg.span,
                                format!("`parse_float64(...)` expects `str`, found `{}`", text_ty),
                            ));
                        }
                        Ok(Type::Named(
                            "Result".to_string(),
                            vec![Type::named("float64"), Type::named("str")],
                        ))
                    }
                }
            }
            ExprKind::Name(name)
                if !locals.contains_key(name) && self.resolve_function_info(name).is_some() =>
            {
                let Some(function) = self.resolve_function_info(name) else {
                    unreachable!("function lookup is stable during call checking");
                };
                let seed_substitutions = if let Some(type_args) = explicit_type_args {
                    self.explicit_type_substitutions(
                        &function.decl.type_params,
                        type_args,
                        span,
                        &format!("function `{}`", name),
                    )?
                } else {
                    HashMap::new()
                };
                self.type_check_callable_args(
                    &format!("function `{}`", name),
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
                )
            }
            ExprKind::Name(name) if self.resolve_class_info(name).is_some() => {
                let class = self
                    .resolve_class_info(name)
                    .expect("class constructor guard should retain the resolved class");
                let class_type_name = self.canonical_class_name(name, class);
                if let Some(constructor) = class.builtin_constructor() {
                    return self.type_check_builtin_class_constructor(
                        constructor,
                        args,
                        span,
                        locals,
                        explicit_type_args.is_some(),
                        Type::named(constructor.qualified_name()),
                    );
                }
                self.type_check_user_class_constructor(
                    class,
                    name,
                    name,
                    class_type_name,
                    explicit_type_args,
                    args,
                    span,
                    locals,
                    expected,
                )
            }
            ExprKind::Name(name) if self.is_builtin_enum_variant_name(name) => {
                let Some(expected_ty) = expected else {
                    return Err(Diagnostic::at(
                        span,
                        "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
                    ));
                };
                let Type::Named(enum_name, _) = expected_ty else {
                    return Err(Diagnostic::at(
                        span,
                        "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
                    ));
                };
                if self
                    .builtin_enum_variant_payload(expected_ty, enum_name, name)
                    .is_none()
                {
                    return Err(Diagnostic::at(
                        span,
                        "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
                    ));
                }
                self.type_check_builtin_enum_variant_constructor(
                    enum_name,
                    name,
                    expected_ty,
                    args,
                    span,
                    locals,
                )
            }
            ExprKind::Member { object, field } => {
                let (base_object, object_type_args) = self.peel_specialization(object);
                if let ExprKind::Name(type_name) = &base_object.kind {
                    if matches!(type_name.as_str(), "list" | "dict" | "set")
                        && !locals.contains_key(type_name)
                    {
                        if let Some(type_args) = object_type_args {
                            let explicit_args = self.lower_explicit_type_args(type_args)?;
                            let expected_arity = if type_name == "dict" { 2 } else { 1 };
                            if explicit_args.len() != expected_arity {
                                return Err(Diagnostic::coded_at(
                                    "AU2002",
                                    object.span,
                                    format!(
                                        "`{type_name}` expects exactly {expected_arity} type argument{}, found {}",
                                        if expected_arity == 1 { "" } else { "s" },
                                        explicit_args.len()
                                    ),
                                ));
                            }
                            let constructor =
                                BuiltinAssociatedFunction::resolve(type_name, field).ok_or_else(
                                    || {
                                        Diagnostic::coded_at(
                                            "AU2001",
                                            span,
                                            format!(
                                                "type `{type_name}` has no associated function `{field}`"
                                            ),
                                        )
                                    },
                                )?;
                            let ordered_args = constructor.bind_args(args, span)?;
                            let minimum = ordered_args[0]
                                .expect("with_capacity binding should retain its required minimum");
                            let actual = self.type_of_expr_hint(
                                &minimum.value,
                                locals,
                                Some(&Type::named("int64")),
                            )?;
                            if actual != Type::named("int64") {
                                return Err(Diagnostic::coded_at(
                                    "AU2002",
                                    minimum.span,
                                    format!(
                                        "`{type_name}.with_capacity` expects `int64`, found `{actual}`"
                                    ),
                                ));
                            }
                            return Ok(Type::Named(type_name.clone(), explicit_args));
                        }
                    }
                    if type_name == "Array" && !locals.contains_key(type_name) {
                        if let Some(type_args) = object_type_args {
                            let explicit_args = self.lower_explicit_type_args(type_args)?;
                            if explicit_args.len() != 1 {
                                return Err(Diagnostic::coded_at(
                                    "AU2002",
                                    object.span,
                                    format!(
                                        "`Array` expects exactly one type argument, found {}",
                                        explicit_args.len()
                                    ),
                                ));
                            }
                            let dtype = explicit_args[0].clone();
                            if !is_array_dtype(&dtype) {
                                return Err(Diagnostic::coded_at(
                                "AU2002",
                                object.span,
                                format!(
                                    "Array dtype must be one of `int32`, `int64`, `float32`, or `float64`, found `{dtype}`"
                                ),
                            ));
                            }
                            let constructor = BuiltinAssociatedFunction::resolve("Array", field)
                                .ok_or_else(|| {
                                    Diagnostic::coded_at(
                                        "AU2001",
                                        span,
                                        format!(
                                            "type `Array` has no associated function `{field}`"
                                        ),
                                    )
                                })?;
                            if explicit_type_args.is_some() {
                                return Err(Diagnostic::coded_at(
                                    "AU2005",
                                    span,
                                    format!(
                                        "`Array.{field}` does not take explicit type arguments"
                                    ),
                                ));
                            }
                            let ordered_args = constructor.bind_args(args, span)?;
                            self.reject_builtin_associated_argument_sibling_overlap(
                                constructor,
                                args,
                                &ordered_args,
                                locals,
                            )?;
                            let shape_ty =
                                Type::Named("list".to_string(), vec![Type::named("int64")]);
                            let array_ty = Type::Named("Array".to_string(), vec![dtype.clone()]);
                            match constructor {
                                BuiltinAssociatedFunction::ArrayZeros => {
                                    let shape = ordered_args[0].expect(
                                        "Array.zeros binding should retain its shape argument",
                                    );
                                    let actual = self.type_of_expr_hint(
                                        &shape.value,
                                        locals,
                                        Some(&shape_ty),
                                    )?;
                                    if actual != shape_ty {
                                        return Err(Diagnostic::coded_at(
                                        "AU2002",
                                        shape.span,
                                        format!(
                                            "`Array.zeros` expects `list[int64]` for `shape`, found `{actual}`"
                                        ),
                                    ));
                                    }
                                }
                                BuiltinAssociatedFunction::ArrayFull => {
                                    for (index, expected_ty, label) in
                                        [(0, &shape_ty, "shape"), (1, &dtype, "value")]
                                    {
                                        let argument = ordered_args[index].expect(
                                            "Array.full binding should retain every argument",
                                        );
                                        let actual = self.type_of_expr_hint(
                                            &argument.value,
                                            locals,
                                            Some(expected_ty),
                                        )?;
                                        if actual != *expected_ty {
                                            return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            argument.span,
                                            format!(
                                                "`Array.full` expects `{expected_ty}` for `{label}`, found `{actual}`"
                                            ),
                                        ));
                                        }
                                    }
                                }
                                BuiltinAssociatedFunction::ArrayFromVec => {
                                    let values_ty =
                                        Type::Named("list".to_string(), vec![dtype.clone()]);
                                    for (index, expected_ty, label) in
                                        [(0, &values_ty, "values"), (1, &shape_ty, "shape")]
                                    {
                                        let argument = ordered_args[index].expect(
                                            "Array.from_list binding should retain every argument",
                                        );
                                        let actual = self.type_of_expr_hint(
                                            &argument.value,
                                            locals,
                                            Some(expected_ty),
                                        )?;
                                        if actual != *expected_ty {
                                            return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            argument.span,
                                            format!(
                                                "`Array.from_list` expects `{expected_ty}` for `{label}`, found `{actual}`"
                                            ),
                                        ));
                                        }
                                    }
                                }
                                BuiltinAssociatedFunction::DurationMilliseconds
                                | BuiltinAssociatedFunction::DurationSeconds
                                | BuiltinAssociatedFunction::DurationMinutes
                                | BuiltinAssociatedFunction::StringFromBytes
                                | BuiltinAssociatedFunction::ListWithCapacity
                                | BuiltinAssociatedFunction::DictWithCapacity
                                | BuiltinAssociatedFunction::SetWithCapacity => unreachable!(
                                    "Array associated lookup returned a non-Array constructor"
                                ),
                            }
                            return Ok(array_ty);
                        }
                    }
                }
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                if method.decl.receiver.is_none() {
                                    let seed_substitutions =
                                        if let Some(type_args) = object_type_args {
                                            self.explicit_type_substitutions(
                                                &class_info.decl.type_params,
                                                type_args,
                                                object.span,
                                                &format!("class `{}`", item_name),
                                            )?
                                        } else {
                                            HashMap::new()
                                        };
                                    return self.type_check_callable_args(
                                        &format!("method `{}`", field),
                                        &method.decl.type_params,
                                        &method.decl.params,
                                        &method.signature.param_passings,
                                        &method.signature.params,
                                        &method.signature.return_type,
                                        &method.type_param_bounds,
                                        &method.signature.rng_clone_safe_type_params,
                                        &method.signature.array_equality_safe_type_params,
                                        args,
                                        span,
                                        locals,
                                        expected,
                                        seed_substitutions,
                                    );
                                }
                            }
                        }
                        if let Some(enum_info) = namespace.enums.get(&item_name) {
                            let enum_type_name =
                                self.module_enum_type_name(&module_path, enum_info);
                            return self.type_check_user_enum_variant_constructor(
                                enum_info,
                                &item_name,
                                enum_type_name,
                                field,
                                object_type_args,
                                args,
                                span,
                                locals,
                                expected,
                            );
                        }
                    }
                }
                if let ExprKind::Name(class_name) = &base_object.kind {
                    if let Some(class_info) = self.resolve_class_info(class_name) {
                        if let Some(method) = class_info.methods.get(field) {
                            if method.decl.receiver.is_some() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "method `{}` on class `{}` requires an instance receiver",
                                        field, class_name
                                    ),
                                ));
                            }
                            let seed_substitutions = if let Some(type_args) = object_type_args {
                                self.explicit_type_substitutions(
                                    &class_info.decl.type_params,
                                    type_args,
                                    object.span,
                                    &format!("class `{}`", class_name),
                                )?
                            } else {
                                HashMap::new()
                            };
                            return self.type_check_callable_args(
                                &format!("method `{}`", field),
                                &method.decl.type_params,
                                &method.decl.params,
                                &method.signature.param_passings,
                                &method.signature.params,
                                &method.signature.return_type,
                                &method.type_param_bounds,
                                &method.signature.rng_clone_safe_type_params,
                                &method.signature.array_equality_safe_type_params,
                                args,
                                span,
                                locals,
                                expected,
                                seed_substitutions,
                            );
                        }
                    }
                }

                if let ExprKind::Name(enum_name) = &base_object.kind {
                    if let Some(type_args) = object_type_args {
                        let explicit_args = self.lower_explicit_type_args(type_args)?;
                        if let Ok(explicit_ty) =
                            self.explicit_builtin_type(enum_name, &explicit_args, span)
                        {
                            return self.type_check_builtin_enum_variant_constructor(
                                enum_name,
                                field,
                                &explicit_ty,
                                args,
                                span,
                                locals,
                            );
                        }
                    }
                    if object_type_args.is_none() && expected.is_none() && enum_name == "Option" {
                        match field.as_str() {
                            "Some" => {
                                if args.len() != 1 {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "variant `{}` of enum `{}` expects 1 payload argument, found {}",
                                            field,
                                            enum_name,
                                            args.len()
                                        ),
                                    ));
                                }
                                self.reject_owned_view_value(
                                    &args[0].value,
                                    locals,
                                    "an enum payload",
                                )?;
                                let actual = self.type_of_expr(&args[0].value, locals)?;
                                if type_contains_loan_closure(&actual) {
                                    return Err(Diagnostic::coded_at(
                                        "AU3010",
                                        args[0].value.span,
                                        "a closure containing a live view cannot be stored in an enum payload",
                                    ));
                                }
                                self.consume_value_expr(&args[0].value, locals)?;
                                return Ok(Type::Named("Option".to_string(), vec![actual]));
                            }
                            "None" => {
                                return Err(Diagnostic::at(
                                    span,
                                    "cannot infer type parameter `T` for enum variant `Option.None`",
                                ));
                            }
                            _ => {}
                        }
                    }
                    if let Some(expected_ty) = expected {
                        if let Some(variant_payloads) =
                            self.builtin_enum_variant_payload(expected_ty, enum_name, field)
                        {
                            if variant_payloads.is_empty() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "variant `{}` of enum `{}` does not take a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            if args.len() != variant_payloads.len() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                                        field,
                                        enum_name,
                                        variant_payloads.len(),
                                        if variant_payloads.len() == 1 { "" } else { "s" },
                                        args.len()
                                    ),
                                ));
                            }
                            for (payload_ty, argument) in variant_payloads.iter().zip(args) {
                                self.reject_owned_view_value(
                                    &argument.value,
                                    locals,
                                    "an enum payload",
                                )?;
                                let actual = self.type_of_expr_hint(
                                    &argument.value,
                                    locals,
                                    Some(payload_ty),
                                )?;
                                if actual != *payload_ty {
                                    return Err(Diagnostic::at(
                                        argument.span,
                                        format!(
                                            "variant `{}` of enum `{}` expects `{}`, found `{}`",
                                            field, enum_name, payload_ty, actual
                                        ),
                                    ));
                                }
                                if type_contains_loan_closure(&actual) {
                                    return Err(Diagnostic::coded_at(
                                        "AU3010",
                                        argument.value.span,
                                        "a closure containing a live view cannot be stored in an enum payload",
                                    ));
                                }
                                self.consume_value_expr(&argument.value, locals)?;
                            }
                            return Ok(expected_ty.clone());
                        }
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        let enum_type_name = self.canonical_enum_info_name(enum_name, enum_info);
                        return self.type_check_user_enum_variant_constructor(
                            enum_info,
                            enum_name,
                            enum_type_name,
                            field,
                            object_type_args,
                            args,
                            span,
                            locals,
                            expected,
                        );
                    }
                }

                let receiver_ty = self.type_of_expr(object, locals)?;
                if let Type::Named(receiver_name, _) = &receiver_ty {
                    if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                        if explicit_type_args.is_some() && builtin_member != BuiltinMember::ArrayMap
                        {
                            return Err(Diagnostic::coded_at(
                                "AU2005",
                                span,
                                format!(
                                    "builtin method `{}.{field}` does not take explicit type arguments",
                                    receiver_name
                                ),
                            ));
                        }
                        self.reject_builtin_member_argument_sibling_overlap(
                            builtin_member,
                            args,
                            locals,
                            span,
                        )?;
                        self.reject_builtin_receiver_argument_overlap(
                            builtin_member,
                            object,
                            &receiver_ty,
                            args,
                            locals,
                        )?;
                    }
                }
                if receiver_ty == Type::named("Duration") {
                    if let Some(
                        builtin_member @ (BuiltinMember::DurationToMilliseconds
                        | BuiltinMember::DurationToSeconds),
                    ) = BuiltinMember::resolve("Duration", field)
                    {
                        builtin_member.bind_args(args, span)?;
                        return Ok(Type::named("float64"));
                    }
                }
                if let Type::Module(module_path) = &receiver_ty {
                    let namespace = self.module_namespace(module_path).ok_or_else(|| {
                        Diagnostic::at(span, format!("unknown module namespace `{}`", module_path))
                    })?;
                    if let Some(function) = namespace.functions.get(field) {
                        let closure_argument_policy =
                            if namespace.path == "control" && field == "retry" {
                                ClosureArgumentPolicy::RepeatableParameter("worker")
                            } else {
                                ClosureArgumentPolicy::Reject
                            };
                        let seed_substitutions = if let Some(type_args) = explicit_type_args {
                            self.explicit_type_substitutions(
                                &function.decl.type_params,
                                type_args,
                                span,
                                &format!("function `{}`", function.decl.name),
                            )?
                        } else {
                            HashMap::new()
                        };
                        return self
                            .type_check_callable_args_detailed(
                                &format!("function `{}`", function.decl.name),
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
                                closure_argument_policy,
                            )
                            .map(|checked| checked.return_type);
                    }
                    if let Some(class) = namespace.classes.get(field) {
                        if let Some(constructor) = class.builtin_constructor() {
                            return self.type_check_builtin_class_constructor(
                                constructor,
                                args,
                                span,
                                locals,
                                explicit_type_args.is_some(),
                                Type::named(constructor.qualified_name()),
                            );
                        }
                        if matches!(
                            (namespace.path.as_str(), class.decl.name.as_str()),
                            ("fs", "File")
                                | ("net", "TcpStream")
                                | ("net", "TcpListener")
                                | ("net", "UdpSocket")
                                | ("net", "UdpDatagram")
                                | ("net", "HttpListener")
                                | ("net", "HttpExchange")
                                | ("net", "HttpResponse")
                                | ("net", "WebSocketListener")
                                | ("net", "WebSocket")
                                | ("net", "UnixListener")
                                | ("net", "UnixStream")
                                | ("net", "TlsListener")
                                | ("net", "TlsStream")
                        ) {
                            return Err(Diagnostic::at(
                                span,
                                format!(
                                    "builtin resource `{}.{}` must be created through its module functions",
                                    namespace.path, class.decl.name
                                ),
                            ));
                        }
                        let class_type_name = format!("{}.{}", namespace.path, class.decl.name);
                        return self.type_check_user_class_constructor(
                            class,
                            &class.decl.name,
                            &class_type_name,
                            class_type_name.clone(),
                            explicit_type_args,
                            args,
                            span,
                            locals,
                            expected,
                        );
                    }
                    return Err(Diagnostic::coded_at(
                        "AU2001",
                        span,
                        format!(
                            "module `{}` has no callable member `{}`",
                            module_path, field
                        ),
                    ));
                }
                if let Type::Named(receiver_name, receiver_args) = &receiver_ty {
                    if receiver_name == "random.Rng" && receiver_args.is_empty() {
                        if field == "clone" {
                            return self
                                .reject_rng_duplication("random.Rng.clone", &receiver_ty, span)
                                .map(|()| Type::Unit);
                        }
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            self.require_mutable_receiver(object, field, span, locals)?;
                            return match builtin_member {
                                BuiltinMember::RngNextInt => {
                                    for (index, label) in [(0, "lo"), (1, "hi")] {
                                        let argument = self.bound_argument(
                                            &ordered_args,
                                            index,
                                            span,
                                            format!("`next_int` requires a `{label}` argument"),
                                        )?;
                                        let actual = self.type_of_expr_hint(
                                            &argument.value,
                                            locals,
                                            Some(&Type::named("int64")),
                                        )?;
                                        if actual != Type::named("int64") {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                argument.span,
                                                format!(
                                                    "`next_int` expects `int64` for `{label}`, found `{actual}`"
                                                ),
                                            ));
                                        }
                                        self.apply_builtin_argument_passing(
                                            builtin_member,
                                            index,
                                            argument,
                                            locals,
                                        )?;
                                    }
                                    Ok(Type::named("int64"))
                                }
                                BuiltinMember::RngNextFloat => Ok(Type::named("float64")),
                                BuiltinMember::RngShuffle => {
                                    let values = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`shuffle` requires a `values` argument",
                                    )?;
                                    let actual = self.type_of_expr(&values.value, locals)?;
                                    if !matches!(
                                        &actual,
                                        Type::Named(name, args)
                                            if name == "list" && args.len() == 1
                                    ) {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            values.span,
                                            format!(
                                                "`shuffle` expects `list[T]`, found `{actual}`"
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        values,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                _ => unreachable!("unexpected random.Rng builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Array" && receiver_args.len() == 1 {
                        let dtype = &receiver_args[0];
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ArrayShape => {
                                    Ok(Type::Named("list".to_string(), vec![Type::named("int64")]))
                                }
                                BuiltinMember::ArrayLen => Ok(Type::named("int64")),
                                BuiltinMember::ArrayClone => Ok(receiver_ty.clone()),
                                BuiltinMember::ArrayGet => {
                                    let index = ordered_args[0].expect(
                                        "Array.get binding should retain its index argument",
                                    );
                                    let expected_index =
                                        Type::Named("list".to_string(), vec![Type::named("int64")]);
                                    let actual = self.type_of_expr_hint(
                                        &index.value,
                                        locals,
                                        Some(&expected_index),
                                    )?;
                                    if actual != expected_index {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            index.span,
                                            format!(
                                                "`Array.get` expects `list[int64]`, found `{actual}`"
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named("Option".to_string(), vec![dtype.clone()]))
                                }
                                BuiltinMember::ArraySet => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let index = ordered_args[0].expect(
                                        "Array.set binding should retain its index argument",
                                    );
                                    let expected_index =
                                        Type::Named("list".to_string(), vec![Type::named("int64")]);
                                    let actual = self.type_of_expr_hint(
                                        &index.value,
                                        locals,
                                        Some(&expected_index),
                                    )?;
                                    if actual != expected_index {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            index.span,
                                            format!(
                                                "`Array.set` expects `list[int64]`, found `{actual}`"
                                            ),
                                        ));
                                    }
                                    let value = ordered_args[1].expect(
                                        "Array.set binding should retain its value argument",
                                    );
                                    let actual =
                                        self.type_of_expr_hint(&value.value, locals, Some(dtype))?;
                                    if actual != *dtype {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            value.span,
                                            format!(
                                                "`Array.set` expects `{dtype}`, found `{actual}`"
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named("Option".to_string(), vec![dtype.clone()]))
                                }
                                BuiltinMember::ArrayFill => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let value = ordered_args[0].expect(
                                        "Array.fill binding should retain its value argument",
                                    );
                                    let actual =
                                        self.type_of_expr_hint(&value.value, locals, Some(dtype))?;
                                    if actual != *dtype {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            value.span,
                                            format!(
                                                "`Array.fill` expects `{dtype}`, found `{actual}`"
                                            ),
                                        ));
                                    }
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::ArrayMap => {
                                    let callback = ordered_args[0]
                                        .expect("Array.map binding should retain its callback");
                                    let output_ty =
                                        self.array_callback_return_type(callback, dtype, locals)?;
                                    if !is_array_dtype(&output_ty) {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            callback.span,
                                            format!(
                                                "Array.map callback must return `int32`, `int64`, `float32`, or `float64`, found `{output_ty}`"
                                            ),
                                        ));
                                    }
                                    if let Some(type_args) = explicit_type_args {
                                        let explicit_outputs =
                                            self.lower_explicit_type_args(type_args)?;
                                        if explicit_outputs.len() != 1 {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                span,
                                                format!(
                                                    "`Array.map` expects exactly one type argument, found {}",
                                                    explicit_outputs.len()
                                                ),
                                            ));
                                        }
                                        let explicit_output = &explicit_outputs[0];
                                        if !is_array_dtype(explicit_output) {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                span,
                                                format!(
                                                    "Array.map output dtype must be one of `int32`, `int64`, `float32`, or `float64`, found `{explicit_output}`"
                                                ),
                                            ));
                                        }
                                        if *explicit_output != output_ty {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                callback.span,
                                                format!(
                                                    "Array.map type argument `{explicit_output}` does not match callback result `{output_ty}`"
                                                ),
                                            ));
                                        }
                                    }
                                    Ok(Type::Named("Array".to_string(), vec![output_ty]))
                                }
                                BuiltinMember::ArraySum
                                | BuiltinMember::ArrayMin
                                | BuiltinMember::ArrayMax => Ok(dtype.clone()),
                                BuiltinMember::ArrayMean => Ok(Type::named("float64")),
                                BuiltinMember::ArrayWrappingAdd
                                | BuiltinMember::ArrayWrappingSub
                                | BuiltinMember::ArrayWrappingMul
                                | BuiltinMember::ArraySaturatingAdd
                                | BuiltinMember::ArraySaturatingSub
                                | BuiltinMember::ArraySaturatingMul => {
                                    if !matches!(
                                        dtype,
                                        Type::Named(name, args)
                                            if args.is_empty()
                                                && matches!(name.as_str(), "int32" | "int64")
                                    ) {
                                        return Err(Diagnostic::coded_at(
                                            "AU2003",
                                            span,
                                            format!(
                                                "`Array.{field}` is available only for integer Arrays"
                                            ),
                                        ));
                                    }
                                    let rhs = ordered_args[0].expect(
                                        "Array integer arithmetic binding should retain its rhs",
                                    );
                                    let rhs_ty = if Self::is_numeric_literal_expr(&rhs.value) {
                                        self.type_of_expr_hint(&rhs.value, locals, Some(dtype))?
                                    } else {
                                        self.type_of_expr(&rhs.value, locals)?
                                    };
                                    if rhs_ty != receiver_ty && rhs_ty != *dtype {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            rhs.span,
                                            format!(
                                                "`Array.{field}` expects `{receiver_ty}` or `{dtype}`, found `{rhs_ty}`"
                                            ),
                                        ));
                                    }
                                    Ok(receiver_ty.clone())
                                }
                                _ => unreachable!("unexpected Array builtin member"),
                            };
                        }
                    }

                    if receiver_name == "list" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::VecLen => Ok(Type::named("int64")),
                                BuiltinMember::VecIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::VecClone => {
                                    self.reject_rng_duplication("list.copy", &receiver_ty, span)?;
                                    Ok(receiver_ty.clone())
                                }
                                BuiltinMember::VecPush => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let push_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`append` requires exactly one argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &push_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            push_arg.span,
                                            format!(
                                                "`append` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        push_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecPop => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    if let Some(index_arg) = ordered_args[0] {
                                        self.check_vec_index_type(
                                            &index_arg.value,
                                            index_arg.span,
                                            locals,
                                        )?;
                                    }
                                    Ok(receiver_args[0].clone())
                                }
                                BuiltinMember::VecGet => {
                                    let index_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`get` requires exactly one argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &index_arg.value,
                                        index_arg.span,
                                        locals,
                                    )?;
                                    self.reject_rng_duplication(
                                        "list.get",
                                        &receiver_args[0],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecSet => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let index_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`set` requires an `index` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &index_arg.value,
                                        index_arg.span,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        index_arg,
                                        locals,
                                    )?;
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`set` requires a `value` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`set` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(receiver_args[0].clone())
                                }
                                BuiltinMember::VecRemove
                                | BuiltinMember::VecIndex
                                | BuiltinMember::VecCount => {
                                    if matches!(builtin_member, BuiltinMember::VecRemove) {
                                        self.require_mutable_receiver(object, field, span, locals)?;
                                    }
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        format!("`{field}` requires exactly one argument"),
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            value_arg.span,
                                            format!(
                                                "`{field}` expects `{}`, found `{actual}`",
                                                receiver_args[0]
                                            ),
                                        ));
                                    }
                                    self.require_array_equality_eligible(
                                        &receiver_args[0],
                                        format!(
                                            "cannot use `list.{field}` with `{}`",
                                            receiver_args[0]
                                        ),
                                        span,
                                    )?;
                                    Ok(
                                        if matches!(
                                            builtin_member,
                                            BuiltinMember::VecIndex | BuiltinMember::VecCount
                                        ) {
                                            Type::named("int64")
                                        } else {
                                            Type::Unit
                                        },
                                    )
                                }
                                BuiltinMember::VecSwap => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let first_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`swap` requires a `first` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &first_arg.value,
                                        first_arg.span,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        first_arg,
                                        locals,
                                    )?;
                                    let second_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`swap` requires a `second` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &second_arg.value,
                                        second_arg.span,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecContains => {
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`contains` requires a `value` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`contains` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.require_array_equality_eligible(
                                        &receiver_args[0],
                                        format!(
                                            "cannot use `list.contains` with `{}`",
                                            receiver_args[0]
                                        ),
                                        span,
                                    )?;
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::VecExtend => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let other_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`extend` requires an `other` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &other_arg.value,
                                        locals,
                                        Some(&receiver_ty),
                                    )?;
                                    if actual != receiver_ty {
                                        return Err(Diagnostic::at(
                                            other_arg.span,
                                            format!(
                                                "`extend` expects `{}`, found `{}`",
                                                receiver_ty, actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        other_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecInsert => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let index_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`insert` requires an `index` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &index_arg.value,
                                        index_arg.span,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        index_arg,
                                        locals,
                                    )?;
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`insert` requires a `value` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`insert` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecClear | BuiltinMember::VecReverse => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecSort => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    if let Some(key_arg) = ordered_args[0] {
                                        let key_ty = self.vec_callback_return_type(
                                            "sort",
                                            key_arg,
                                            &receiver_args[0],
                                            locals,
                                        )?;
                                        self.require_vec_orderable(
                                            "sort",
                                            "key type",
                                            &key_ty,
                                            key_arg.span,
                                        )?;
                                        self.apply_builtin_argument_passing(
                                            builtin_member,
                                            0,
                                            key_arg,
                                            locals,
                                        )?;
                                    } else {
                                        self.require_vec_orderable(
                                            "sort",
                                            "list element type",
                                            &receiver_args[0],
                                            span,
                                        )?;
                                    }
                                    if let Some(reverse_arg) = ordered_args[1] {
                                        let actual = self.type_of_expr_hint(
                                            &reverse_arg.value,
                                            locals,
                                            Some(&Type::named("bool")),
                                        )?;
                                        if actual != Type::named("bool") {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                reverse_arg.span,
                                                format!(
                                                    "`sort` expects `bool` for `reverse`, found `{actual}`"
                                                ),
                                            ));
                                        }
                                    }
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecMap => {
                                    let callback = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`map` requires an `f` argument",
                                    )?;
                                    let output_ty = self.vec_callback_return_type(
                                        "map",
                                        callback,
                                        &receiver_args[0],
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        callback,
                                        locals,
                                    )?;
                                    Ok(Type::Named("list".to_string(), vec![output_ty]))
                                }
                                BuiltinMember::VecFilter => {
                                    let callback = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`filter` requires an `f` argument",
                                    )?;
                                    let output_ty = self.vec_callback_return_type(
                                        "filter",
                                        callback,
                                        &receiver_args[0],
                                        locals,
                                    )?;
                                    if output_ty != Type::named("bool") {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            callback.span,
                                            format!(
                                                "`list.filter` callback must return `bool`, found `{output_ty}`"
                                            ),
                                        ));
                                    }
                                    self.reject_rng_duplication(
                                        "list.filter",
                                        &receiver_args[0],
                                        span,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        callback,
                                        locals,
                                    )?;
                                    Ok(receiver_ty.clone())
                                }
                                BuiltinMember::VecReserve => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let additional = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`reserve` requires an `additional` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &additional.value,
                                        locals,
                                        Some(&Type::named("int64")),
                                    )?;
                                    if actual != Type::named("int64") {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            additional.span,
                                            format!("`reserve` expects `int64`, found `{actual}`"),
                                        ));
                                    }
                                    Ok(Type::Unit)
                                }
                                _ => unreachable!("unexpected vector builtin member"),
                            };
                        }
                    }

                    if receiver_name == "str" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::StringLen | BuiltinMember::StringByteLen => {
                                    Ok(Type::named("int64"))
                                }
                                BuiltinMember::StringToBytes => {
                                    Ok(Type::Named("list".to_string(), vec![Type::named("uint8")]))
                                }
                                BuiltinMember::StringContains
                                | BuiltinMember::StringStartsWith
                                | BuiltinMember::StringEndsWith => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "string predicate methods require one argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("str")),
                                    )?;
                                    if actual != Type::named("str") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`{}` expects `str`, found `{}`",
                                                field, actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::StringSplit => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`split` requires a `text` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("str")),
                                    )?;
                                    if actual != Type::named("str") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!("`split` expects `str`, found `{}`", actual),
                                        ));
                                    }
                                    Ok(Type::Named("list".to_string(), vec![Type::named("str")]))
                                }
                                BuiltinMember::StringReplace => {
                                    let from_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`replace` requires a `from` argument",
                                    )?;
                                    let from_actual = self.type_of_expr_hint(
                                        &from_arg.value,
                                        locals,
                                        Some(&Type::named("str")),
                                    )?;
                                    if from_actual != Type::named("str") {
                                        return Err(Diagnostic::at(
                                            from_arg.span,
                                            format!(
                                                "`replace` expects `str` for `from`, found `{}`",
                                                from_actual
                                            ),
                                        ));
                                    }
                                    let to_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`replace` requires a `to` argument",
                                    )?;
                                    let to_actual = self.type_of_expr_hint(
                                        &to_arg.value,
                                        locals,
                                        Some(&Type::named("str")),
                                    )?;
                                    if to_actual != Type::named("str") {
                                        return Err(Diagnostic::at(
                                            to_arg.span,
                                            format!(
                                                "`replace` expects `str` for `to`, found `{}`",
                                                to_actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("str"))
                                }
                                BuiltinMember::StringToLower
                                | BuiltinMember::StringToUpper
                                | BuiltinMember::StringTrim
                                | BuiltinMember::StringClone => Ok(Type::named("str")),
                                BuiltinMember::StringJoin => {
                                    let parts_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`join` requires a `parts` argument",
                                    )?;
                                    let expected_parts =
                                        Type::Named("list".to_string(), vec![Type::named("str")]);
                                    let actual = self.type_of_expr_hint(
                                        &parts_arg.value,
                                        locals,
                                        Some(&expected_parts),
                                    )?;
                                    if actual != expected_parts {
                                        return Err(Diagnostic::at(
                                            parts_arg.span,
                                            format!(
                                                "`join` expects `list[str]`, found `{}`",
                                                actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("str"))
                                }
                                BuiltinMember::StringStripPrefix
                                | BuiltinMember::StringStripSuffix => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "string strip methods require one `text` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("str")),
                                    )?;
                                    if actual != Type::named("str") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`{}` expects `str`, found `{}`",
                                                field, actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named("Option".to_string(), vec![Type::named("str")]))
                                }
                                _ => unreachable!("unexpected string builtin member"),
                            };
                        }
                    }

                    if receiver_name == "dict" && receiver_args.len() == 2 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            if matches!(
                                builtin_member,
                                BuiltinMember::MapGet
                                    | BuiltinMember::MapSet
                                    | BuiltinMember::MapRemove
                                    | BuiltinMember::MapContainsKey
                            ) {
                                self.require_array_equality_eligible(
                                    &receiver_args[0],
                                    format!(
                                        "cannot use `dict.{field}` with `{}`",
                                        receiver_args[0]
                                    ),
                                    span,
                                )?;
                            }
                            return match builtin_member {
                                BuiltinMember::MapLen => Ok(Type::named("int64")),
                                BuiltinMember::MapIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::MapClone => {
                                    self.reject_rng_duplication("dict.copy", &receiver_ty, span)?;
                                    Ok(receiver_ty.clone())
                                }
                                BuiltinMember::MapGet => {
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`get` requires exactly one key argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`get` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.reject_rng_duplication(
                                        "dict.get",
                                        &receiver_args[1],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapSet => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`set` requires a `key` argument",
                                    )?;
                                    let key_actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if key_actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`set` expects key type `{}`, found `{}`",
                                                receiver_args[0], key_actual
                                            ),
                                        ));
                                    }
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`set` requires a `value` argument",
                                    )?;
                                    let value_actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[1]),
                                    )?;
                                    if value_actual != receiver_args[1] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`set` expects value type `{}`, found `{}`",
                                                receiver_args[1], value_actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        key_arg,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapRemove => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`remove` requires exactly one key argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`remove` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapContainsKey => {
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`contains` requires exactly one key argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`contains` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::MapKeys => {
                                    self.reject_rng_duplication(
                                        "dict.keys",
                                        &receiver_args[0],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "list".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::MapValues => {
                                    self.reject_rng_duplication(
                                        "dict.values",
                                        &receiver_args[1],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "list".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapItems => {
                                    let entry_type = Type::Tuple(vec![
                                        receiver_args[0].clone(),
                                        receiver_args[1].clone(),
                                    ]);
                                    self.reject_rng_duplication("dict.items", &entry_type, span)?;
                                    Ok(Type::Named("list".to_string(), vec![entry_type]))
                                }
                                BuiltinMember::MapClear => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::MapExtend => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let other_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`update` requires an `other` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &other_arg.value,
                                        locals,
                                        Some(&receiver_ty),
                                    )?;
                                    if actual != receiver_ty {
                                        return Err(Diagnostic::at(
                                            other_arg.span,
                                            format!(
                                                "`update` expects `{}`, found `{}`",
                                                receiver_ty, actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        other_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::MapReserve => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let additional = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`reserve` requires an `additional` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &additional.value,
                                        locals,
                                        Some(&Type::named("int64")),
                                    )?;
                                    if actual != Type::named("int64") {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            additional.span,
                                            format!("`reserve` expects `int64`, found `{actual}`"),
                                        ));
                                    }
                                    Ok(Type::Unit)
                                }
                                _ => unreachable!("unexpected map builtin member"),
                            };
                        }
                    }

                    if receiver_name == "set" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            if matches!(
                                builtin_member,
                                BuiltinMember::SetContains
                                    | BuiltinMember::SetInsert
                                    | BuiltinMember::SetRemove
                                    | BuiltinMember::SetDiscard
                            ) {
                                self.require_array_equality_eligible(
                                    &receiver_args[0],
                                    format!("cannot use `set.{field}` with `{}`", receiver_args[0]),
                                    span,
                                )?;
                            }
                            return match builtin_member {
                                BuiltinMember::SetLen => Ok(Type::named("int64")),
                                BuiltinMember::SetIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::SetClone => {
                                    self.reject_rng_duplication("set.copy", &receiver_ty, span)?;
                                    Ok(receiver_ty.clone())
                                }
                                BuiltinMember::SetContains => {
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`contains` requires a `value` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`contains` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::SetInsert
                                | BuiltinMember::SetRemove
                                | BuiltinMember::SetDiscard => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "set mutation requires a `value` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`{}` expects `{}`, found `{}`",
                                                field, receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::SetClear => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::SetReserve => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let additional = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`reserve` requires an `additional` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &additional.value,
                                        locals,
                                        Some(&Type::named("int64")),
                                    )?;
                                    if actual != Type::named("int64") {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            additional.span,
                                            format!("`reserve` expects `int64`, found `{actual}`"),
                                        ));
                                    }
                                    Ok(Type::Unit)
                                }
                                _ => unreachable!("unexpected set builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Queue" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::QueuePut | BuiltinMember::QueueTryPut => {
                                    self.require_queue_payload_transfer(&receiver_args[0], span)?;
                                    let send_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        format!("`{}` requires exactly one argument", field),
                                    )?;
                                    let direct_view = self
                                        .borrow_call_place(&send_arg.value)
                                        .and_then(|place| locals.get(&place.root))
                                        .is_some_and(|binding| binding.view.is_some());
                                    if direct_view
                                        || self
                                            .returned_view_call_kind(&send_arg.value, locals)?
                                            .is_some()
                                    {
                                        return Err(Diagnostic::coded_at(
                                            "AU3008",
                                            send_arg.span,
                                            "a view cannot be inserted into a Queue, even when its pointee type is Transfer",
                                        )
                                        .with_help(
                                            "send an owned copy or clone whose lifetime is independent of the view",
                                        ));
                                    }
                                    let actual = self.type_of_expr_hint(
                                        &send_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            send_arg.span,
                                            format!(
                                                "`{}` expects `{}`, found `{}`",
                                                field, receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        send_arg,
                                        locals,
                                    )?;
                                    if matches!(builtin_member, BuiltinMember::QueuePut) {
                                        if let Some(timeout_arg) =
                                            ordered_args.get(1).and_then(|arg| *arg)
                                        {
                                            let actual = self.type_of_expr_hint(
                                                &timeout_arg.value,
                                                locals,
                                                Some(&Type::named("Duration")),
                                            )?;
                                            if actual != Type::named("Duration") {
                                                return Err(Diagnostic::at(
                                                    timeout_arg.span,
                                                    format!(
                                                        "`put(timeout=...)` expects `Duration`, found `{}`",
                                                        actual
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            Type::Named(
                                                "SendError".to_string(),
                                                vec![receiver_args[0].clone()],
                                            ),
                                        ],
                                    ))
                                }
                                BuiltinMember::QueueGet | BuiltinMember::QueueGetOrNone => {
                                    if let Some(timeout_arg) = ordered_args[0] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`get(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    if matches!(builtin_member, BuiltinMember::QueueGetOrNone) {
                                        Ok(Type::Named(
                                            "Option".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    } else {
                                        Ok(Type::Named(
                                            "QueueReceive".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    }
                                }
                                BuiltinMember::QueueGetOr => {
                                    let default_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`get_or` requires a `default` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &default_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            default_arg.span,
                                            format!(
                                                "`get_or` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        default_arg,
                                        locals,
                                    )?;
                                    if let Some(timeout_arg) = ordered_args[1] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`get_or(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    Ok(receiver_args[0].clone())
                                }
                                BuiltinMember::QueueClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected queue builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Task" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            self.reject_rng_duplication(
                                &format!("Task.{field}"),
                                &receiver_args[0],
                                span,
                            )?;
                            let task_ty =
                                Type::Named("Task".to_string(), vec![receiver_args[0].clone()]);
                            self.reject_conditional_value_argument_overlap(
                                object,
                                &task_ty,
                                "Task result receiver",
                                args,
                                None,
                                locals,
                            )?;
                            return match builtin_member {
                                BuiltinMember::TaskResult | BuiltinMember::TaskResultOrNone => {
                                    if let Some(timeout_arg) = ordered_args[0] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`result(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    self.consume_task_observation_right(
                                        object,
                                        &receiver_args[0],
                                        field,
                                        locals,
                                    )?;
                                    if matches!(builtin_member, BuiltinMember::TaskResultOrNone) {
                                        Ok(Type::Named(
                                            "Option".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    } else {
                                        Ok(Type::Named(
                                            "TaskResult".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    }
                                }
                                BuiltinMember::TaskResultOr => {
                                    let default_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`result_or` requires a `default` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &default_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            default_arg.span,
                                            format!(
                                                "`result_or` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        default_arg,
                                        locals,
                                    )?;
                                    if let Some(timeout_arg) = ordered_args[1] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`result_or(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    self.consume_task_observation_right(
                                        object,
                                        &receiver_args[0],
                                        field,
                                        locals,
                                    )?;
                                    Ok(receiver_args[0].clone())
                                }
                                _ => unreachable!("unexpected task builtin member"),
                            };
                        }
                    }

                    if receiver_name == "TaskGroup" && receiver_args.is_empty() {
                        match field.as_str() {
                            "start"
                            | "start_soon"
                            | "start_with_stack"
                            | "start_soon_with_stack" => {
                                let has_stack_override = matches!(
                                    field.as_str(),
                                    "start_with_stack" | "start_soon_with_stack"
                                );
                                let target_index = usize::from(has_stack_override);
                                let required_count = target_index + 1;
                                if args.len() < required_count {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "`{}` expects {}a target function followed by its arguments",
                                            field,
                                            if has_stack_override {
                                                "a stack size in bytes and "
                                            } else {
                                                ""
                                            },
                                        ),
                                    ));
                                }
                                if let Some(argument) = args[..required_count]
                                    .iter()
                                    .find(|argument| argument.name.is_some())
                                {
                                    return Err(Diagnostic::at(
                                        argument.span,
                                        format!("`{}` does not take keyword arguments", field),
                                    ));
                                }
                                if has_stack_override {
                                    let actual = self.type_of_expr_hint(
                                        &args[0].value,
                                        locals,
                                        Some(&Type::named("int64")),
                                    )?;
                                    if actual != Type::named("int64") {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            args[0].span,
                                            format!(
                                                "`{}` stack size expects `int64`, found `{}`",
                                                field, actual
                                            ),
                                        ));
                                    }
                                    let literal_bytes = match &args[0].value.kind {
                                        ExprKind::Int(bytes) => Some((*bytes, false)),
                                        ExprKind::Unary {
                                            op: UnaryOp::Neg,
                                            expr,
                                        } => match &expr.kind {
                                            ExprKind::Int(bytes) => Some((*bytes, true)),
                                            _ => None,
                                        },
                                        _ => None,
                                    };
                                    if let Some((magnitude, negative)) = literal_bytes {
                                        let outside_bounds = negative
                                            || magnitude
                                                < u128::try_from(crate::call::MIN_TASK_STACK_BYTES)
                                                    .expect("minimum stack size is positive")
                                            || magnitude
                                                > u128::try_from(crate::call::MAX_TASK_STACK_BYTES)
                                                    .expect("maximum stack size is positive");
                                        if outside_bounds {
                                            let found = if negative {
                                                format!("-{magnitude}")
                                            } else {
                                                magnitude.to_string()
                                            };
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                args[0].span,
                                                format!(
                                                    "task stack size must be between {} and {} bytes, found {}",
                                                    crate::call::MIN_TASK_STACK_BYTES,
                                                    crate::call::MAX_TASK_STACK_BYTES,
                                                    found
                                                ),
                                            ));
                                        }
                                    }
                                }
                                let callable = match self
                                    .resolve_spawn_callable(&args[target_index].value)
                                {
                                    Ok(callable) => callable,
                                    Err(named_target_error) => {
                                        if named_target_error.code == "AU2999"
                                            && named_target_error
                                                .message
                                                .starts_with("extern function `")
                                        {
                                            return Err(named_target_error);
                                        }
                                        let target_ty =
                                            self.type_of_expr(&args[target_index].value, locals)?;
                                        let (params, return_type, closure_captures) =
                                            match &target_ty {
                                                Type::Function {
                                                    params,
                                                    return_type,
                                                } => (params.clone(), return_type.clone(), None),
                                                Type::Closure {
                                                    params,
                                                    return_type,
                                                    captures,
                                                    ..
                                                } => (
                                                    params.as_ref().clone(),
                                                    return_type.clone(),
                                                    Some(captures.as_slice()),
                                                ),
                                                _ => return Err(named_target_error),
                                            };
                                        if let Some(captures) = closure_captures {
                                            if let Some(reason) = self.transfer_failure(&target_ty)
                                            {
                                                let target_label =
                                                    match &args[target_index].value.kind {
                                                        ExprKind::Name(name) => {
                                                            format!("task target `{name}`")
                                                        }
                                                        _ => "task closure target".to_string(),
                                                    };
                                                let mut diagnostic = Diagnostic::coded_at(
                                                    "AU3008",
                                                    args[target_index].span,
                                                    format!(
                                                        "{target_label} cannot cross a task boundary because {reason}"
                                                    ),
                                                );
                                                if let Some(capture) =
                                                    captures.iter().find(|capture| {
                                                        self.transfer_failure(&capture.ty).is_some()
                                                    })
                                                {
                                                    diagnostic = diagnostic.with_secondary(
                                                        capture.span,
                                                        format!(
                                                            "capture `{}` is created here",
                                                            capture.name
                                                        ),
                                                    );
                                                }
                                                return Err(diagnostic.with_help(
                                                    "task closures may capture only owned Transfer data; keep capabilities and host resources on their owning worker",
                                                ));
                                            }
                                        };
                                        if let Some(index) = params.iter().position(|param| {
                                            param.passing == ReceiverKind::BorrowMut
                                        }) {
                                            return Err(Diagnostic::coded_at(
                                                "AU3002",
                                                args[target_index].span,
                                                format!(
                                                    "task starting does not support mutable parameter {} on a function value; child tasks cannot write back through the starting call frame",
                                                    index + 1,
                                                ),
                                            ));
                                        }
                                        let spawn_args = &args[required_count..];
                                        let capture_params = params
                                            .iter()
                                            .map(|param| FunctionParamContract {
                                                keyword_only: param.keyword_only,
                                                name: param.name.clone(),
                                                ty: param.ty.clone(),
                                                passing: ReceiverKind::Value,
                                                has_default: param.has_default,
                                                default_erased: param.default_erased,
                                            })
                                            .collect::<Vec<_>>();
                                        let checked_return = self.type_check_function_value_args(
                                            &capture_params,
                                            &return_type,
                                            spawn_args,
                                            span,
                                            locals,
                                            None,
                                        )?;
                                        for (index, (param, argument)) in
                                            params.iter().zip(spawn_args).enumerate()
                                        {
                                            self.require_transfer(
                                                &param.ty,
                                                format!(
                                                    "task argument {} for function value",
                                                    index + 1
                                                ),
                                                argument.span,
                                            )?;
                                        }
                                        self.require_transfer(
                                            &checked_return,
                                            format!("task result `{checked_return}`"),
                                            args[target_index].span,
                                        )?;
                                        if closure_captures.is_some() {
                                            self.consume_value_expr(
                                                &args[target_index].value,
                                                locals,
                                            )?;
                                        }
                                        return Ok(
                                            if matches!(
                                                field.as_str(),
                                                "start" | "start_with_stack"
                                            ) {
                                                Type::Named(
                                                    "Task".to_string(),
                                                    vec![checked_return],
                                                )
                                            } else {
                                                Type::Unit
                                            },
                                        );
                                    }
                                };
                                self.require_task_startable_function(
                                    &callable.display_name,
                                    &callable.decl.params,
                                    &callable.signature.param_passings,
                                    args[target_index].span,
                                )?;
                                let spawn_args = &args[required_count..];
                                for (index, argument) in spawn_args.iter().enumerate() {
                                    if self
                                        .direct_view_value_kind(&argument.value, locals)?
                                        .is_some()
                                    {
                                        return Err(Diagnostic::coded_at(
                                            "AU3008",
                                            argument.span,
                                            format!(
                                                "view argument {} cannot cross a task boundary",
                                                index + 1
                                            ),
                                        )
                                        .with_help(
                                            "pass an explicit owned copy or clone whose lifetime is independent of the view",
                                        ));
                                    }
                                }
                                let capture_passings = vec![
                                    ReceiverKind::Value;
                                    callable.signature.param_passings.len()
                                ];
                                let checked_callable = self.type_check_callable_args_detailed(
                                    &callable.display_name,
                                    &callable.decl.type_params,
                                    &callable.decl.params,
                                    &capture_passings,
                                    &callable.signature.params,
                                    &callable.signature.return_type,
                                    &callable.type_param_bounds,
                                    &callable.signature.rng_clone_safe_type_params,
                                    &callable.signature.array_equality_safe_type_params,
                                    spawn_args,
                                    span,
                                    locals,
                                    None,
                                    callable.seed_substitutions,
                                    ClosureArgumentPolicy::Reject,
                                )?;
                                let transfer_args = bind_call_arguments(
                                    &callable.display_name,
                                    &callable_params_from_decl(&callable.decl.params),
                                    spawn_args,
                                    span,
                                    CallConvention::PositionalOrNamed,
                                )?;
                                for (index, (argument, param)) in transfer_args
                                    .into_iter()
                                    .zip(callable.decl.params.iter())
                                    .enumerate()
                                {
                                    let value_span = argument
                                        .map(|argument| argument.span)
                                        .or_else(|| {
                                            param.default.as_ref().map(|default| default.span)
                                        })
                                        .unwrap_or(param.span);
                                    if argument.is_some_and(|argument| {
                                        self.borrow_call_place(&argument.value)
                                            .and_then(|place| locals.get(&place.root))
                                            .is_some_and(|binding| binding.view.is_some())
                                    }) {
                                        return Err(Diagnostic::coded_at(
                                            "AU3008",
                                            value_span,
                                            format!(
                                                "view argument `{}` cannot cross a task boundary",
                                                param.name
                                            ),
                                        )
                                        .with_help(
                                            "pass an explicit owned copy or clone whose lifetime is independent of the view",
                                        ));
                                    }
                                    let concrete_param =
                                        checked_callable.params.get(index).ok_or_else(|| {
                                            Diagnostic::at(
                                                span,
                                                "internal error: task parameter type is missing",
                                            )
                                        })?;
                                    self.require_transfer(
                                        concrete_param,
                                        format!("task argument `{}`", param.name),
                                        value_span,
                                    )
                                    .map_err(|diagnostic| {
                                        diagnostic.with_secondary(
                                            param.span,
                                            format!(
                                                "task parameter `{}` is declared here",
                                                param.name
                                            ),
                                        )
                                    })?;
                                }
                                self.require_transfer(
                                    &checked_callable.return_type,
                                    format!("task result `{}`", checked_callable.return_type),
                                    args[target_index].span,
                                )
                                .map_err(|diagnostic| {
                                    diagnostic.with_secondary(
                                        callable.decl.return_type.span,
                                        "task target return type is declared here",
                                    )
                                })?;
                                return Ok(
                                    if matches!(field.as_str(), "start" | "start_with_stack") {
                                        Type::Named(
                                            "Task".to_string(),
                                            vec![checked_callable.return_type],
                                        )
                                    } else {
                                        Type::Unit
                                    },
                                );
                            }
                            "cancel" => {
                                BuiltinMember::TaskGroupCancel.bind_args(args, span)?;
                                return Ok(Type::Unit);
                            }
                            _ => {}
                        }
                    }

                    if receiver_name == "fs.File" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::FileReadAll => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::FileReadBytes => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![bytes_ty.clone(), crate::builtin_modules::io_error_type()],
                                )),
                                BuiltinMember::FileWriteAll => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_all` requires a `text` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("str")),
                                    )?;
                                    if actual != Type::named("str") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`write_all` expects `str`, found `{}`",
                                                actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::FileWriteBytes => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_bytes` requires a `bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "write_bytes",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::FileFlush => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                )),
                                BuiltinMember::FileClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected file builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Child" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessChildStdin
                                | BuiltinMember::ProcessChildStdout
                                | BuiltinMember::ProcessChildStderr => Ok(Type::Named(
                                    "Option".to_string(),
                                    vec![Type::named("process.Pipe")],
                                )),
                                BuiltinMember::ProcessChildWait => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait(timeout=...)",
                                    )?;
                                    Ok(Type::named("process.Wait"))
                                }
                                BuiltinMember::ProcessChildWaitOrNone => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait_or_none(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("process.ExitStatus")],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessChildWaitOk => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait_ok(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("process.ExitStatus"),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessChildKill
                                | BuiltinMember::ProcessChildTerminate => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                BuiltinMember::ProcessChildClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected process child builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Pipe" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessPipeReadAll => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::process_error_type(),
                                    ],
                                )),
                                BuiltinMember::ProcessPipeReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("str")],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeReadBytes => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`read_bytes` requires a `max_bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_bytes",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        count_arg,
                                        locals,
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![bytes_ty.clone()],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_all` requires a `text` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("str"),
                                        locals,
                                        "write_all",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeWriteBytes => {
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_bytes` requires a `bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "write_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeFlush => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                BuiltinMember::ProcessPipeClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected process pipe builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Completed" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessCompletedStatus => {
                                    Ok(Type::named("process.ExitStatus"))
                                }
                                BuiltinMember::ProcessCompletedSuccess => Ok(Type::named("bool")),
                                BuiltinMember::ProcessCompletedStdout
                                | BuiltinMember::ProcessCompletedStderr => Ok(Type::named("str")),
                                BuiltinMember::ProcessCompletedStdoutBytes
                                | BuiltinMember::ProcessCompletedStderrBytes => {
                                    Ok(Type::Named("list".to_string(), vec![Type::named("uint8")]))
                                }
                                BuiltinMember::ProcessCompletedCheck => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                _ => unreachable!("unexpected process completed builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Supervisor" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessSupervisorStart => {
                                    self.check_builtin_argument_type(
                                        self.bound_argument(
                                            &ordered_args,
                                            0,
                                            span,
                                            "`start` requires a `name` argument",
                                        )?,
                                        &Type::named("str"),
                                        locals,
                                        "start",
                                    )?;
                                    self.check_builtin_argument_type(
                                        self.bound_argument(
                                            &ordered_args,
                                            1,
                                            span,
                                            "`start` requires a `command` argument",
                                        )?,
                                        &Type::Named("list".to_string(), vec![Type::named("str")]),
                                        locals,
                                        "start",
                                    )?;
                                    if let Some(argument) = ordered_args.get(2).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("str")],
                                            ),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(3).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::Named(
                                                "dict".to_string(),
                                                vec![Type::named("str"), Type::named("str")],
                                            ),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(4).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("process.Stdio"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(5).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("process.Stdio"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(6).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("process.Stdio"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(7).copied().flatten() {
                                        let expected = Type::named("process.RestartPolicy");
                                        let actual = self.type_of_expr_hint(
                                            &argument.value,
                                            locals,
                                            Some(&expected),
                                        )?;
                                        if actual != expected
                                            && actual != Type::named("RestartPolicy")
                                        {
                                            return Err(Diagnostic::at(
                                                argument.span,
                                                format!(
                                                    "`start` expects `process.RestartPolicy`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    if let Some(argument) = ordered_args.get(8).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("Duration"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(9).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("int32"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(10).copied().flatten()
                                    {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("bool"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    for (index, argument) in ordered_args.iter().enumerate() {
                                        if let Some(argument) = *argument {
                                            self.apply_builtin_argument_passing(
                                                builtin_member,
                                                index,
                                                argument,
                                                locals,
                                            )?;
                                        }
                                    }
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessSupervisorWait => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait(timeout=...)",
                                    )?;
                                    Ok(Type::named("process.SupervisorWait"))
                                }
                                BuiltinMember::ProcessSupervisorWaitOrNone => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait_or_none(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("process.SupervisorEvent")],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessSupervisorStop => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                BuiltinMember::ProcessSupervisorIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::ProcessSupervisorClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected process supervisor builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TcpListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TcpListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.TcpStream"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::TcpListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tcp listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TcpStream" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TcpStreamReadAll => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("str"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("str")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamReadBytes => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`read_bytes` requires a `max_bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_bytes",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        count_arg,
                                        locals,
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![bytes_ty.clone()],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamReadExact => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`read_exact` requires a `count` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_exact",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        count_arg,
                                        locals,
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_exact(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            bytes_ty.clone(),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_all` requires a `text` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("str")),
                                    )?;
                                    if actual != Type::named("str") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`write_all` expects `str`, found `{}`",
                                                actual
                                            ),
                                        ));
                                    }
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TcpStreamWriteBytes => {
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_bytes` requires a `bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "write_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TcpStreamFlush
                                | BuiltinMember::TcpStreamLocalAddr
                                | BuiltinMember::TcpStreamPeerAddr
                                | BuiltinMember::TcpStreamShutdownRead
                                | BuiltinMember::TcpStreamShutdownWrite
                                | BuiltinMember::TcpStreamShutdownBoth => {
                                    let value_ty = if matches!(
                                        builtin_member,
                                        BuiltinMember::TcpStreamFlush
                                            | BuiltinMember::TcpStreamShutdownRead
                                            | BuiltinMember::TcpStreamShutdownWrite
                                            | BuiltinMember::TcpStreamShutdownBoth
                                    ) {
                                        Type::Unit
                                    } else {
                                        Type::named("str")
                                    };
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![value_ty, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TcpStreamClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tcp stream builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UdpSocket" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::UdpSocketSendText => {
                                    let address_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        address_arg,
                                        &Type::named("str"),
                                        locals,
                                        "send_text",
                                    )?;
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("str"),
                                        locals,
                                        "send_text",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        2,
                                        locals,
                                        "send_text(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UdpSocketSendBytes => {
                                    let address_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        address_arg,
                                        &Type::named("str"),
                                        locals,
                                        "send_bytes",
                                    )?;
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "send_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        2,
                                        locals,
                                        "send_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UdpSocketRecv => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "recv",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        count_arg,
                                        locals,
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "recv(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![bytes_ty.clone()],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UdpSocketRecvFrom => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "recv_from",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        count_arg,
                                        locals,
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "recv_from(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("net.UdpDatagram")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UdpSocketLocalAddr
                                | BuiltinMember::UdpSocketPeerAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::UdpSocketClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected udp socket builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UdpDatagram" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            return match builtin_member {
                                BuiltinMember::UdpDatagramAddress => Ok(Type::named("str")),
                                BuiltinMember::UdpDatagramBytes => Ok(bytes_ty),
                                BuiltinMember::UdpDatagramText => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                _ => unreachable!("unexpected udp datagram builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.HttpListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::HttpListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.HttpExchange"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::HttpListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::HttpListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected http listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.HttpExchange" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let headers_ty = Type::Named(
                                "dict".to_string(),
                                vec![Type::named("str"), Type::named("str")],
                            );
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::HttpExchangeMethod
                                | BuiltinMember::HttpExchangePath => Ok(Type::named("str")),
                                BuiltinMember::HttpExchangeHeaders => Ok(headers_ty),
                                BuiltinMember::HttpExchangeBodyText => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::HttpExchangeBodyBytes => Ok(bytes_ty),
                                BuiltinMember::HttpExchangeRespondText => {
                                    let status_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        status_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "respond_text",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        status_arg,
                                        locals,
                                    )?;
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("str"),
                                        locals,
                                        "respond_text",
                                    )?;
                                    let headers_arg = self.bound_argument(
                                        &ordered_args,
                                        2,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        headers_arg,
                                        &headers_ty,
                                        locals,
                                        "respond_text",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        text_arg,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        2,
                                        headers_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::HttpExchangeRespondBytes => {
                                    let status_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        status_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "respond_bytes",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        status_arg,
                                        locals,
                                    )?;
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "respond_bytes",
                                    )?;
                                    let headers_arg = self.bound_argument(
                                        &ordered_args,
                                        2,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        headers_arg,
                                        &headers_ty,
                                        locals,
                                        "respond_bytes",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        bytes_arg,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        2,
                                        headers_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                _ => unreachable!("unexpected http exchange builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.HttpResponse" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let headers_ty = Type::Named(
                                "dict".to_string(),
                                vec![Type::named("str"), Type::named("str")],
                            );
                            return match builtin_member {
                                BuiltinMember::HttpResponseStatus => Ok(Type::named("int32")),
                                BuiltinMember::HttpResponseReason => Ok(Type::named("str")),
                                BuiltinMember::HttpResponseHeaders => Ok(headers_ty),
                                BuiltinMember::HttpResponseText => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::HttpResponseBytes => Ok(bytes_ty),
                                _ => unreachable!("unexpected http response builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.WebSocketListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::WebSocketListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.WebSocket"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::WebSocketListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                _ => unreachable!("unexpected websocket listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.WebSocket" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::WebSocketSendText => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("str"),
                                        locals,
                                        "send_text",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "send_text(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::WebSocketSendBytes => {
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "send_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "send_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::WebSocketRecvText => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "recv_text(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("str")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::WebSocketRecvBytes => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "recv_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named("Option".to_string(), vec![bytes_ty]),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::WebSocketClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected websocket builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UnixListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::UnixListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.UnixStream"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UnixListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected unix listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UnixStream" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::UnixStreamReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("str")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UnixStreamReadExact => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_exact",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        count_arg,
                                        locals,
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_exact(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![bytes_ty, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UnixStreamWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("str"),
                                        locals,
                                        "write_all",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UnixStreamClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected unix stream builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TlsListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TlsListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.TlsStream"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TlsListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("str"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::TlsListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tls listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TlsStream" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("list".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TlsStreamReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("str")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TlsStreamReadExact => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_exact",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        count_arg,
                                        locals,
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_exact(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![bytes_ty, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TlsStreamWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("str"),
                                        locals,
                                        "write_all",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TlsStreamClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tls stream builtin member"),
                            };
                        }
                    }
                }

                if let Type::Named(class_name, type_args) = &receiver_ty {
                    if let Some(class_info) = self.resolve_class_info(class_name) {
                        if let Some(method) = class_info.methods.get(field) {
                            if self.is_external_module(&class_info.module_name)
                                && !method.decl.public
                            {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "method `{}` is private on `{}`",
                                        field, class_info.decl.name
                                    ),
                                ));
                            }
                            if method.decl.receiver.is_none() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "associated method `{}` on class `{}` must be called through the class name",
                                        field, class_name
                                    ),
                                ));
                            }
                            let receiver_borrows = self.prepare_method_receiver_borrows(
                                field,
                                method.decl.receiver,
                                object,
                                span,
                                locals,
                            )?;
                            return self
                                .type_check_callable_args_seeded(
                                    &format!("method `{}`", field),
                                    &method.decl.type_params,
                                    &method.decl.params,
                                    &method.signature.param_passings,
                                    &method.signature.params,
                                    &method.signature.return_type,
                                    &method.type_param_bounds,
                                    &method.signature.rng_clone_safe_type_params,
                                    &method.signature.array_equality_safe_type_params,
                                    args,
                                    span,
                                    locals,
                                    expected,
                                    substitutions_from_decl_type_args(
                                        &class_info.decl.type_params,
                                        type_args,
                                    ),
                                    receiver_borrows,
                                    ClosureArgumentPolicy::Reject,
                                )
                                .map(|checked| checked.return_type);
                        }
                    }
                }
                if let Type::TypeParam(type_param_name) = &receiver_ty {
                    if let Ok(method) = self.trait_method_from_type_param(type_param_name, field) {
                        self.enforce_resolved_rng_clone_obligations_before_method_inference(
                            &format!("method `{}`", field),
                            &method.rng_clone_safe_types,
                            &method.decl.type_params,
                            span,
                        )?;
                        self.enforce_resolved_array_equality_obligations_before_method_inference(
                            &format!("method `{}`", field),
                            &method.array_equality_safe_types,
                            span,
                        )?;
                        let receiver_borrows = self.prepare_method_receiver_borrows(
                            field,
                            method.decl.receiver,
                            object,
                            span,
                            locals,
                        )?;
                        return self
                            .type_check_callable_args_seeded(
                                &format!("method `{}`", field),
                                &method.decl.type_params,
                                &method.decl.params,
                                &method.signature.param_passings,
                                &method.signature.params,
                                &method.signature.return_type,
                                &method.type_param_bounds,
                                &method.signature.rng_clone_safe_type_params,
                                &method.signature.array_equality_safe_type_params,
                                args,
                                span,
                                locals,
                                expected,
                                HashMap::new(),
                                receiver_borrows,
                                ClosureArgumentPolicy::Reject,
                            )
                            .map(|checked| checked.return_type);
                    }
                }
                if let Some((_trait_impl, method, impl_substitutions)) =
                    self.trait_method_for_concrete_type(&receiver_ty, field, span)?
                {
                    self.enforce_rng_clone_obligations_before_method_inference(
                        &format!("method `{}`", field),
                        &method.signature.rng_clone_safe_type_params,
                        &impl_substitutions,
                        &method.decl.type_params,
                        span,
                    )?;
                    let substituted_params = method
                        .signature
                        .params
                        .iter()
                        .map(|param| substitute_type(param, &impl_substitutions))
                        .collect::<Vec<_>>();
                    let substituted_return_type =
                        substitute_type(&method.signature.return_type, &impl_substitutions);
                    let receiver_borrows = self.prepare_method_receiver_borrows(
                        field,
                        method.decl.receiver,
                        object,
                        span,
                        locals,
                    )?;
                    return self
                        .type_check_callable_args_seeded(
                            &format!("method `{}`", field),
                            &method.decl.type_params,
                            &method.decl.params,
                            &method.signature.param_passings,
                            &substituted_params,
                            &substituted_return_type,
                            &method.type_param_bounds,
                            &method.signature.rng_clone_safe_type_params,
                            &method.signature.array_equality_safe_type_params,
                            args,
                            span,
                            locals,
                            expected,
                            impl_substitutions,
                            receiver_borrows,
                            ClosureArgumentPolicy::Reject,
                        )
                        .map(|checked| checked.return_type);
                }
                if let Ok(Type::Function {
                    params,
                    return_type,
                }) = self.resolve_member_type(&receiver_ty, field, span)
                {
                    if explicit_type_args.is_some() {
                        return Err(Diagnostic::coded_at(
                            "AU2005",
                            span,
                            "function values have a concrete signature and do not take explicit type arguments",
                        ));
                    }
                    return self.type_check_function_value_args(
                        &params,
                        &return_type,
                        args,
                        span,
                        locals,
                        expected,
                    );
                }
                match (&receiver_ty, field.as_str()) {
                    (Type::Named(name, type_args), method_name)
                        if type_args.is_empty()
                            && is_integer_type(&receiver_ty)
                            && matches!(
                                method_name,
                                "wrapping_add"
                                    | "wrapping_sub"
                                    | "wrapping_mul"
                                    | "saturating_add"
                                    | "saturating_sub"
                                    | "saturating_mul"
                                    | "wrapping_shl"
                                    | "wrapping_shr"
                                    | "saturating_shl"
                                    | "saturating_shr"
                            ) =>
                    {
                        let builtin = BuiltinMember::resolve(name, method_name)
                            .expect("integer arithmetic method should resolve");
                        let ordered_args = builtin.bind_args(args, span)?;
                        let argument_name =
                            if method_name.ends_with("shl") || method_name.ends_with("shr") {
                                "count"
                            } else {
                                "rhs"
                            };
                        let rhs = self.bound_argument(
                            &ordered_args,
                            0,
                            span,
                            format!("`{method_name}` requires a `{argument_name}` argument"),
                        )?;
                        let actual =
                            self.type_of_expr_hint(&rhs.value, locals, Some(&receiver_ty))?;
                        if actual != receiver_ty {
                            return Err(Diagnostic::coded_at(
                                "AU2002",
                                rhs.span,
                                format!(
                                    "`{method_name}` expects `{receiver_ty}`, found `{actual}`"
                                ),
                            ));
                        }
                        Ok(receiver_ty)
                    }
                    (Type::Named(_name, type_args), "to_float")
                        if type_args.is_empty() && is_integer_type(&receiver_ty) =>
                    {
                        BuiltinMember::IntegerToFloat.bind_args(args, span)?;
                        Ok(Type::named("float64"))
                    }
                    (Type::Named(name, type_args), "to_string")
                        if type_args.is_empty()
                            && (is_numeric_type(&receiver_ty) || name == "bool") =>
                    {
                        BuiltinMember::ScalarToString.bind_args(args, span)?;
                        Ok(Type::named("str"))
                    }
                    (Type::Named(name, type_args), "sqrt")
                        if type_args.is_empty() && name == "float64" =>
                    {
                        BuiltinMember::FloatSqrt.bind_args(args, span)?;
                        Ok(Type::named("float64"))
                    }
                    _ => Err(Diagnostic::at(
                        span,
                        format!("unsupported method call `{}` on `{}`", field, receiver_ty),
                    )),
                }
            }
            _ => {
                if matches!(
                    &base_callee.kind,
                    ExprKind::Name(name) if !locals.contains_key(name)
                ) {
                    return Err(self.unsupported_call_target_diagnostic(callee, span));
                }
                let callee_ty = self.type_of_expr(base_callee, locals)?;
                match callee_ty {
                    Type::Function {
                        params,
                        return_type,
                    } => self.type_check_function_value_args(
                        &params,
                        &return_type,
                        args,
                        span,
                        locals,
                        expected,
                    ),
                    Type::Closure {
                        params,
                        return_type,
                        call_kind,
                        ..
                    } => {
                        if explicit_type_args.is_some() {
                            return Err(Diagnostic::coded_at(
                                "AU2005",
                                span,
                                "closure values have a concrete signature and do not take explicit type arguments",
                            ));
                        }
                        if call_kind == ClosureCallKind::Consuming {
                            self.consume_value_expr(base_callee, locals)?;
                        } else if call_kind == ClosureCallKind::MutableRepeatable
                            && !self.is_mutable_place(base_callee, locals)?
                        {
                            return Err(Diagnostic::coded_at(
                                "AU3003",
                                base_callee.span,
                                "a mutable-repeatable closure must be called through a mutable closure place",
                            )
                            .with_help("store the closure in a `mut` local before calling it"));
                        }
                        self.type_check_function_value_args(
                            &params,
                            &return_type,
                            args,
                            span,
                            locals,
                            expected,
                        )
                    }
                    _ => Err(self.unsupported_call_target_diagnostic(callee, span)),
                }
            }
        }
    }

    fn check_match(
        &self,
        match_stmt: &MatchStmt,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        let control_last_uses = locals
            .keys()
            .filter_map(|name| {
                last_name_reference_span_in_match(match_stmt, name).map(|span| (name.clone(), span))
            })
            .collect::<BTreeMap<_, _>>();
        let shared_scrutinee =
            self.shared_match_scrutinee_name(&match_stmt.scrutinee, match_stmt.capability);
        let shared_match_place = (match_stmt.capability == ReceiverKind::Borrow)
            .then(|| self.shared_match_place(&match_stmt.scrutinee, locals))
            .flatten();
        let active_match_borrow = if match_stmt.capability == ReceiverKind::BorrowMut {
            self.begin_match_borrow_mut(&match_stmt.scrutinee, match_stmt.span, locals)?
        } else {
            None
        };
        let result = (|| {
            let scrutinee_ty = self.type_of_expr(&match_stmt.scrutinee, locals)?;
            if match_stmt.capability == ReceiverKind::Value {
                if self.is_copy_type(&scrutinee_ty) {
                    if let Some(place) = self.borrow_call_place(&match_stmt.scrutinee) {
                        self.ensure_place_not_shared_by_match_for_move(
                            &place,
                            match_stmt.scrutinee.span,
                            locals,
                        )?;
                    }
                } else {
                    self.consume_match_scrutinee_expr(&match_stmt.scrutinee, locals)?;
                }
            }

            if match_stmt.arms.is_empty() {
                return Err(Diagnostic::at(
                    match_stmt.span,
                    "`match` requires at least one `case` arm",
                ));
            }

            if let Some(variants) = self.enum_variants_for_type(&scrutinee_ty) {
                let Type::Named(enum_name, _type_args) = &scrutinee_ty else {
                    unreachable!("enum scrutinee types should be named");
                };
                let scrutinee_enum_name = self.canonical_enum_name(enum_name);
                let mut covered = BTreeMap::<String, crate::diag::Span>::new();
                let mut patterns_by_variant =
                    BTreeMap::<String, Vec<crate::ast::VariantPattern>>::new();
                let mut wildcard_span = None;
                let mut all_return = true;
                let mut arm_states = Vec::new();

                for (index, arm) in match_stmt.arms.iter().enumerate() {
                    let mut arm_locals = locals.clone();
                    if self.is_copy_type(&scrutinee_ty) {
                        if let Some(place) = shared_match_place.as_ref() {
                            self.retain_shared_match_place(place, match_stmt.span, &mut arm_locals);
                        }
                    }
                    match &arm.pattern {
                        Pattern::Or(pattern) => {
                            self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                match_stmt.capability,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                        Pattern::Wildcard(span) => {
                            if arm.guard.is_none() {
                                if index + 1 != match_stmt.arms.len() {
                                    return Err(Diagnostic::at(
                                        *span,
                                        "wildcard match arm must be the final `case`",
                                    ));
                                }
                                wildcard_span = Some(*span);
                            }
                        }
                        Pattern::Literal(pattern) => {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                "match over `{}` expects enum variant patterns, not literal `{}`",
                                enum_name,
                                self.render_literal_pattern(pattern)
                            ),
                            ));
                        }
                        Pattern::Binding(binding) => {
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                match_stmt.capability,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                            if arm.guard.is_none() {
                                if index + 1 != match_stmt.arms.len() {
                                    return Err(Diagnostic::at(
                                        binding.span,
                                        "catch-all match arm must be the final `case`",
                                    ));
                                }
                                wildcard_span = Some(binding.span);
                            }
                        }
                        Pattern::Tuple(tuple) => {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "match over `{}` expects enum variant patterns, not a tuple pattern",
                                    enum_name
                                ),
                            ));
                        }
                        Pattern::Variant(pattern) => {
                            let pattern_enum_name =
                                if let Some(pattern_enum_name) = &pattern.enum_name {
                                    if pattern_enum_name == enum_name {
                                        pattern_enum_name.clone()
                                    } else if let Some(pattern_enum_info) =
                                        self.resolve_enum_info(pattern_enum_name)
                                    {
                                        pattern_enum_info.decl.name.clone()
                                    } else {
                                        return Err(Diagnostic::at(
                                            pattern.span,
                                            format!(
                                                "unknown enum `{}` in match pattern",
                                                pattern_enum_name
                                            ),
                                        ));
                                    }
                                } else {
                                    scrutinee_enum_name.clone()
                                };
                            if pattern_enum_name != scrutinee_enum_name {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "match arm expects enum `{}`, found pattern for `{}`",
                                        scrutinee_enum_name, pattern_enum_name
                                    ),
                                ));
                            }

                            let Some(variant_payload) = variants
                                .iter()
                                .find(|(name, _)| name == &pattern.variant_name)
                                .map(|(_, payload)| payload.clone())
                            else {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "enum `{}` has no variant `{}`",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            };

                            let covers_entire_variant = arm.guard.is_none()
                                && self.variant_pattern_covers_payloads(pattern, &variant_payload);
                            if covers_entire_variant {
                                if let Some(previous) =
                                    covered.insert(pattern.variant_name.clone(), pattern.span)
                                {
                                    return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "duplicate match arm for `{}.{}` (previously matched at {})",
                                        scrutinee_enum_name, pattern.variant_name, previous
                                    ),
                                ));
                                }
                            }
                            if arm.guard.is_none() {
                                patterns_by_variant
                                    .entry(pattern.variant_name.clone())
                                    .or_default()
                                    .push(pattern.clone());
                            }

                            if pattern.subpatterns.is_empty() && !variant_payload.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` carries a payload and must bind it",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if variant_payload.is_empty() && !pattern.subpatterns.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` does not carry a payload",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if pattern.subpatterns.len() != variant_payload.len() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` expects {} pattern payload{}, found {}",
                                        scrutinee_enum_name,
                                        pattern.variant_name,
                                        variant_payload.len(),
                                        if variant_payload.len() == 1 { "" } else { "s" },
                                        pattern.subpatterns.len()
                                    ),
                                ));
                            }
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                match_stmt.capability,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                    }

                    let prior_patterns = match_stmt.arms[..index]
                        .iter()
                        .filter(|previous_arm| previous_arm.guard.is_none())
                        .map(|previous_arm| &previous_arm.pattern)
                        .collect::<Vec<_>>();
                    if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::at(
                            self.pattern_span(&arm.pattern),
                            "unreachable match arm",
                        ));
                    }

                    self.check_match_guard(
                        arm.guard.as_ref(),
                        &arm.pattern,
                        match_stmt.capability,
                        &mut arm_locals,
                    )?;
                    self.expire_views_unused_in_branch(
                        &arm.body,
                        &control_last_uses,
                        &mut arm_locals,
                    );
                    let arm_flow = self.check_block(
                        &arm.body,
                        &mut arm_locals,
                        return_type,
                        loop_depth,
                        allow_return,
                    )?;
                    if arm_flow != BlockFlow::AlwaysReturns {
                        all_return = false;
                        arm_states.push(arm_locals);
                    }
                }

                for (variant_name, payloads) in &variants {
                    if covered.contains_key(variant_name) {
                        continue;
                    }
                    let Some(patterns) = patterns_by_variant.get(variant_name) else {
                        continue;
                    };
                    let pattern_refs = patterns.iter().collect::<Vec<_>>();
                    if self.variant_patterns_cover_payloads_union(&pattern_refs, payloads) {
                        let span = patterns
                            .first()
                            .map(|pattern| pattern.span)
                            .unwrap_or(match_stmt.span);
                        covered.insert(variant_name.clone(), span);
                    }
                }

                let branch_states = arm_states.iter().collect::<Vec<_>>();
                self.merge_control_flow_moves(locals, &branch_states);

                let pattern_refs = match_stmt
                    .arms
                    .iter()
                    .filter(|arm| arm.guard.is_none())
                    .map(|arm| &arm.pattern)
                    .collect::<Vec<_>>();
                let missing = self.missing_patterns_for_type(&pattern_refs, &scrutinee_ty);
                if wildcard_span.is_none() && !missing.is_empty() {
                    let rendered = missing
                        .iter()
                        .map(|name| format!("`{}`", name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(Diagnostic::at(
                        match_stmt.span,
                        format!(
                            "non-exhaustive match over `{}`: missing {}",
                            enum_name, rendered
                        ),
                    ));
                }

                return if all_return {
                    Ok(BlockFlow::AlwaysReturns)
                } else {
                    Ok(BlockFlow::FallsThrough)
                };
            }

            let class_scrutinee = match &scrutinee_ty {
                Type::Named(name, _) => self.resolve_class_info(name).is_some(),
                _ => false,
            };
            if class_scrutinee {
                if let Some(pattern) = match_stmt
                    .arms
                    .iter()
                    .map(|arm| &arm.pattern)
                    .find(|pattern| pattern_contains_variant_shape(pattern))
                {
                    return Err(Diagnostic::coded_at(
                        "AU2999",
                        self.pattern_span(pattern),
                        "class patterns are not supported; match an explicit enum/tag representation or use a wildcard and ordinary code",
                    ));
                }
            }

            if !matches!(scrutinee_ty, Type::Tuple(_) | Type::Named(_, _))
                || !(matches!(scrutinee_ty, Type::Tuple(_))
                    || is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                    || is_string_type(&scrutinee_ty)
                    || class_scrutinee)
            {
                return Err(Diagnostic::at(
                match_stmt.span,
                format!(
                    "`match` currently requires a tuple, enum, bool, integer, float, or str scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
            }

            let mut wildcard_span = None;
            let mut all_return = true;
            let mut covered_literals = BTreeMap::<LiteralPatternKey, crate::diag::Span>::new();
            let mut covered_bools = BTreeSet::<bool>::new();
            let mut arm_states = Vec::new();

            for (index, arm) in match_stmt.arms.iter().enumerate() {
                let mut arm_locals = locals.clone();
                if self.is_copy_type(&scrutinee_ty) {
                    if let Some(place) = shared_match_place.as_ref() {
                        self.retain_shared_match_place(place, match_stmt.span, &mut arm_locals);
                    }
                }
                match &arm.pattern {
                    Pattern::Or(pattern) => {
                        self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.capability,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                    Pattern::Wildcard(span) => {
                        if arm.guard.is_none() {
                            if index + 1 != match_stmt.arms.len() {
                                return Err(Diagnostic::at(
                                    *span,
                                    "wildcard match arm must be the final `case`",
                                ));
                            }
                            wildcard_span = Some(*span);
                        }
                    }
                    Pattern::Literal(pattern) => {
                        let key = self.literal_pattern_key(pattern, &scrutinee_ty)?;
                        if let Some(previous) = arm
                            .guard
                            .is_none()
                            .then(|| covered_literals.insert(key.clone(), pattern.span))
                            .flatten()
                        {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                "duplicate match arm for literal `{}` (previously matched at {})",
                                render_literal_pattern_key(&key),
                                previous
                            ),
                            ));
                        }
                        if arm.guard.is_none() {
                            if let LiteralPatternKey::Bool(value) = key {
                                covered_bools.insert(value);
                            }
                        }
                    }
                    Pattern::Variant(pattern) => {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "match over `{}` only supports literal patterns and `_`",
                                scrutinee_ty
                            ),
                        ));
                    }
                    Pattern::Binding(binding) => {
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.capability,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                        if arm.guard.is_none() {
                            if index + 1 != match_stmt.arms.len() {
                                return Err(Diagnostic::at(
                                    binding.span,
                                    "catch-all match arm must be the final `case`",
                                ));
                            }
                            wildcard_span = Some(binding.span);
                        }
                    }
                    Pattern::Tuple(tuple) => {
                        if !matches!(scrutinee_ty, Type::Tuple(_)) {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "tuple pattern requires a tuple scrutinee, found `{}`",
                                    scrutinee_ty
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.capability,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                }

                let prior_patterns = match_stmt.arms[..index]
                    .iter()
                    .filter(|previous_arm| previous_arm.guard.is_none())
                    .map(|previous_arm| &previous_arm.pattern)
                    .collect::<Vec<_>>();
                if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                    return Err(Diagnostic::at(
                        self.pattern_span(&arm.pattern),
                        "unreachable match arm",
                    ));
                }

                self.check_match_guard(
                    arm.guard.as_ref(),
                    &arm.pattern,
                    match_stmt.capability,
                    &mut arm_locals,
                )?;
                self.expire_views_unused_in_branch(&arm.body, &control_last_uses, &mut arm_locals);
                let arm_flow = self.check_block(
                    &arm.body,
                    &mut arm_locals,
                    return_type,
                    loop_depth,
                    allow_return,
                )?;
                if arm_flow != BlockFlow::AlwaysReturns {
                    all_return = false;
                    arm_states.push(arm_locals);
                }
            }

            if wildcard_span.is_none() {
                if matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                {
                    let missing = [true, false]
                        .into_iter()
                        .filter(|value| !covered_bools.contains(value))
                        .map(|value| format!("`{}`", value))
                        .collect::<Vec<_>>();
                    if !missing.is_empty() {
                        return Err(Diagnostic::at(
                            match_stmt.span,
                            format!(
                                "non-exhaustive match over `bool`: missing {}",
                                missing.join(", ")
                            ),
                        ));
                    }
                } else if matches!(scrutinee_ty, Type::Tuple(_)) {
                    let patterns = match_stmt
                        .arms
                        .iter()
                        .filter(|arm| arm.guard.is_none())
                        .map(|arm| &arm.pattern)
                        .collect::<Vec<_>>();
                    if !self
                        .missing_patterns_for_type(&patterns, &scrutinee_ty)
                        .is_empty()
                    {
                        return Err(Diagnostic::at(
                            match_stmt.span,
                            format!(
                                "non-exhaustive match over `{}`: add a covering tuple pattern or final `case _:`",
                                scrutinee_ty
                            ),
                        ));
                    }
                } else {
                    return Err(Diagnostic::at(
                        match_stmt.span,
                        format!(
                        "`match` over `{}` with literal patterns requires a final `case _:` arm",
                        scrutinee_ty
                    ),
                    ));
                }
            }

            let branch_states = arm_states.iter().collect::<Vec<_>>();
            self.merge_control_flow_moves(locals, &branch_states);

            if all_return {
                Ok(BlockFlow::AlwaysReturns)
            } else {
                Ok(BlockFlow::FallsThrough)
            }
        })();
        self.end_match_borrow_mut(active_match_borrow);
        result
    }

    fn bind_pattern_locals(
        &self,
        pattern: &Pattern,
        expected_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        borrow_mode: ReceiverKind,
        match_borrow_place: Option<&PlacePath>,
        shared_match_scrutinee: Option<&str>,
    ) -> Result<()> {
        match pattern {
            Pattern::Or(pattern) => {
                let original = locals.clone();
                let mut canonical: Option<HashMap<String, LocalBinding>> = None;
                for alternative in &pattern.alternatives {
                    let mut alternative_locals = original.clone();
                    self.bind_pattern_locals(
                        alternative,
                        expected_ty,
                        &mut alternative_locals,
                        borrow_mode,
                        match_borrow_place,
                        shared_match_scrutinee,
                    )?;
                    let added = alternative_locals
                        .iter()
                        .filter(|(name, _)| !original.contains_key(*name))
                        .map(|(name, binding)| (name.clone(), binding.clone()))
                        .collect::<HashMap<_, _>>();
                    if let Some(expected) = &canonical {
                        if expected.len() != added.len()
                            || expected.iter().any(|(name, binding)| {
                                added.get(name).is_none_or(|actual| {
                                    actual.ty != binding.ty || actual.passing != binding.passing
                                })
                            })
                        {
                            return Err(Diagnostic::coded_at(
                                "AU2999",
                                pattern.span,
                                "every alternative in an or-pattern must bind the same names with identical types and capabilities",
                            ));
                        }
                    } else {
                        canonical = Some(added);
                    }
                }
                if let Some(bindings) = canonical {
                    locals.extend(bindings);
                }
                Ok(())
            }
            Pattern::Wildcard(_) => Ok(()),
            Pattern::Literal(pattern) => {
                let _ = self.literal_pattern_key(pattern, expected_ty)?;
                Ok(())
            }
            Pattern::Binding(binding) => {
                if locals.contains_key(&binding.name) {
                    return Err(Diagnostic::at(
                        binding.span,
                        format!(
                            "pattern binding `{}` would shadow an existing name",
                            binding.name
                        ),
                    ));
                }
                let passing = if self.is_copy_type(expected_ty) {
                    ReceiverKind::Value
                } else {
                    borrow_mode
                };
                locals.insert(
                    binding.name.clone(),
                    LocalBinding {
                        ty: expected_ty.clone(),
                        assignable: borrow_mode == ReceiverKind::BorrowMut,
                        mutable_place: borrow_mode == ReceiverKind::BorrowMut,
                        managed_resource: false,
                        passing,
                        borrow_origin: None,
                        borrowed_at: (borrow_mode != ReceiverKind::Value).then_some(binding.span),
                        match_borrow_place: (borrow_mode == ReceiverKind::BorrowMut
                            || passing != ReceiverKind::Value)
                            .then(|| match_borrow_place.cloned())
                            .flatten(),
                        stale_match_borrow_place: None,
                        shared_match_scrutinee: (passing == ReceiverKind::Borrow)
                            .then(|| shared_match_scrutinee.map(str::to_string))
                            .flatten(),
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
                Ok(())
            }
            Pattern::Tuple(tuple_pattern) => {
                if borrow_mode == ReceiverKind::BorrowMut {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        tuple_pattern.span,
                        "`match mut` does not support tuple patterns; bind the tuple as one mutable name",
                    ));
                }
                let Type::Tuple(element_types) = expected_ty else {
                    return Err(Diagnostic::at(
                        tuple_pattern.span,
                        format!(
                            "tuple pattern requires a tuple scrutinee, found `{}`",
                            expected_ty
                        ),
                    ));
                };
                if tuple_pattern.elements.len() != element_types.len() {
                    return Err(Diagnostic::at(
                        tuple_pattern.span,
                        format!(
                            "tuple pattern has {} elements but the scrutinee has {}",
                            tuple_pattern.elements.len(),
                            element_types.len()
                        ),
                    ));
                }
                for (element, element_ty) in tuple_pattern.elements.iter().zip(element_types) {
                    self.bind_pattern_locals(
                        element,
                        element_ty,
                        locals,
                        borrow_mode,
                        match_borrow_place,
                        shared_match_scrutinee,
                    )?;
                }
                Ok(())
            }
            Pattern::Variant(variant_pattern) => {
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "pattern `{}` expects an enum scrutinee, found `{}`",
                            variant_pattern.variant_name, expected_ty
                        ),
                    ));
                };
                let Type::Named(enum_name, _) = expected_ty else {
                    unreachable!("enum pattern scrutinee types should be named");
                };
                let expected_enum_name = self.canonical_enum_name(enum_name);
                let pattern_enum_name = variant_pattern
                    .enum_name
                    .as_deref()
                    .map(|name| self.canonical_enum_name(name))
                    .unwrap_or_else(|| expected_enum_name.clone());
                if pattern_enum_name != expected_enum_name {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "match arm expects enum `{}`, found pattern for `{}`",
                            expected_enum_name, pattern_enum_name
                        ),
                    ));
                }
                let Some((_, payloads)) = variants
                    .iter()
                    .find(|(name, _)| name == &variant_pattern.variant_name)
                else {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "enum `{}` has no variant `{}`",
                            enum_name, variant_pattern.variant_name
                        ),
                    ));
                };
                if variant_pattern.subpatterns.is_empty() && !payloads.is_empty() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` carries a payload and must bind it",
                            expected_enum_name, variant_pattern.variant_name
                        ),
                    ));
                }
                if payloads.is_empty() && !variant_pattern.subpatterns.is_empty() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` does not carry a payload",
                            expected_enum_name, variant_pattern.variant_name
                        ),
                    ));
                }
                if payloads.len() != variant_pattern.subpatterns.len() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` expects {} pattern payload{}, found {}",
                            expected_enum_name,
                            variant_pattern.variant_name,
                            payloads.len(),
                            if payloads.len() == 1 { "" } else { "s" },
                            variant_pattern.subpatterns.len()
                        ),
                    ));
                }
                for (subpattern, payload_ty) in
                    variant_pattern.subpatterns.iter().zip(payloads.iter())
                {
                    self.bind_pattern_locals(
                        subpattern,
                        payload_ty,
                        locals,
                        borrow_mode,
                        match_borrow_place,
                        shared_match_scrutinee,
                    )?;
                }
                Ok(())
            }
        }
    }

    /// Types one `match` arm value the way the surrounding expression uses the
    /// match result.
    fn type_of_match_arm_value(
        &self,
        value: &Expr,
        arm_locals: &mut HashMap<String, LocalBinding>,
        result_ty: Option<&Type>,
        result_use: BranchResultUse<'_>,
    ) -> Result<Type> {
        match result_use {
            BranchResultUse::ProjectedField(field) => Ok(self
                .type_member_result_consuming(value, field.name, field.span, arm_locals, result_ty)?
                .0),
            BranchResultUse::Consumed => {
                self.type_expr_consuming_result(value, arm_locals, result_ty)
            }
            BranchResultUse::Inspected => match result_ty {
                Some(expected_ty) => self.type_of_expr_hint(value, arm_locals, Some(expected_ty)),
                None => self.type_of_expr(value, arm_locals),
            },
        }
    }

    fn type_of_match_expr(
        &self,
        parts: MatchExprParts<'_>,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
        result_use: BranchResultUse<'_>,
    ) -> Result<Type> {
        let MatchExprParts {
            scrutinee,
            borrow_mode,
            arms,
            span,
        } = parts;
        let shared_scrutinee = self.shared_match_scrutinee_name(scrutinee, borrow_mode);
        let shared_match_place = (borrow_mode == ReceiverKind::Borrow)
            .then(|| self.shared_match_place(scrutinee, locals))
            .flatten();
        let active_match_borrow = if borrow_mode == ReceiverKind::BorrowMut {
            self.begin_match_borrow_mut(scrutinee, span, locals)?
        } else {
            None
        };
        let result = (|| {
            let scrutinee_ty = self.type_of_expr(scrutinee, locals)?;
            if borrow_mode == ReceiverKind::Value {
                if self.is_copy_type(&scrutinee_ty) {
                    if let Some(place) = self.borrow_call_place(scrutinee) {
                        self.ensure_place_not_shared_by_match_for_move(
                            &place,
                            scrutinee.span,
                            locals,
                        )?;
                    }
                } else {
                    self.consume_match_scrutinee_expr(scrutinee, locals)?;
                }
            }
            if arms.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    "`match` requires at least one `case` arm",
                ));
            }

            let mut result_ty = expected.cloned();

            if let Some(variants) = self.enum_variants_for_type(&scrutinee_ty) {
                let Type::Named(enum_name, _) = &scrutinee_ty else {
                    unreachable!("enum scrutinee types should be named");
                };
                let scrutinee_enum_name = self.canonical_enum_name(enum_name);
                let mut covered = BTreeSet::<String>::new();
                let mut patterns_by_variant =
                    BTreeMap::<String, Vec<crate::ast::VariantPattern>>::new();
                let mut wildcard_seen = false;
                let mut arm_states = Vec::new();

                for (index, arm) in arms.iter().enumerate() {
                    let mut arm_locals = locals.clone();
                    if self.is_copy_type(&scrutinee_ty) {
                        if let Some(place) = shared_match_place.as_ref() {
                            self.retain_shared_match_place(place, span, &mut arm_locals);
                        }
                    }
                    match &arm.pattern {
                        Pattern::Or(pattern) => {
                            self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                borrow_mode,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                        Pattern::Wildcard(wildcard_span) => {
                            if arm.guard.is_none() {
                                if index + 1 != arms.len() {
                                    return Err(Diagnostic::at(
                                        *wildcard_span,
                                        "wildcard match arm must be the final `case`",
                                    ));
                                }
                                wildcard_seen = true;
                            }
                        }
                        Pattern::Literal(pattern) => {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                "match over `{}` expects enum variant patterns, not literal `{}`",
                                enum_name,
                                self.render_literal_pattern(pattern)
                            ),
                            ));
                        }
                        Pattern::Binding(binding) => {
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                borrow_mode,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                            if arm.guard.is_none() {
                                if index + 1 != arms.len() {
                                    return Err(Diagnostic::at(
                                        binding.span,
                                        "catch-all match arm must be the final `case`",
                                    ));
                                }
                                wildcard_seen = true;
                            }
                        }
                        Pattern::Tuple(tuple) => {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "match over `{}` expects enum variant patterns, not a tuple pattern",
                                    enum_name
                                ),
                            ));
                        }
                        Pattern::Variant(pattern) => {
                            let pattern_enum_name =
                                if let Some(pattern_enum_name) = &pattern.enum_name {
                                    if pattern_enum_name == enum_name {
                                        pattern_enum_name.clone()
                                    } else if let Some(pattern_enum_info) =
                                        self.resolve_enum_info(pattern_enum_name)
                                    {
                                        pattern_enum_info.decl.name.clone()
                                    } else {
                                        return Err(Diagnostic::at(
                                            pattern.span,
                                            format!(
                                                "unknown enum `{}` in match pattern",
                                                pattern_enum_name
                                            ),
                                        ));
                                    }
                                } else {
                                    scrutinee_enum_name.clone()
                                };
                            if pattern_enum_name != scrutinee_enum_name {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "match arm expects enum `{}`, found pattern for `{}`",
                                        scrutinee_enum_name, pattern_enum_name
                                    ),
                                ));
                            }

                            let Some(variant_payload) = variants
                                .iter()
                                .find(|(name, _)| name == &pattern.variant_name)
                                .map(|(_, payload)| payload.clone())
                            else {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "enum `{}` has no variant `{}`",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            };
                            if arm.guard.is_none()
                                && self.variant_pattern_covers_payloads(pattern, &variant_payload)
                            {
                                covered.insert(pattern.variant_name.clone());
                            }
                            if arm.guard.is_none() {
                                patterns_by_variant
                                    .entry(pattern.variant_name.clone())
                                    .or_default()
                                    .push(pattern.clone());
                            }

                            if pattern.subpatterns.is_empty() && !variant_payload.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` carries a payload and must bind it",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if variant_payload.is_empty() && !pattern.subpatterns.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` does not carry a payload",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if pattern.subpatterns.len() != variant_payload.len() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` expects {} pattern payload{}, found {}",
                                        scrutinee_enum_name,
                                        pattern.variant_name,
                                        variant_payload.len(),
                                        if variant_payload.len() == 1 { "" } else { "s" },
                                        pattern.subpatterns.len()
                                    ),
                                ));
                            }
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                borrow_mode,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                    }

                    let prior_patterns = arms[..index]
                        .iter()
                        .filter(|previous_arm| previous_arm.guard.is_none())
                        .map(|previous_arm| &previous_arm.pattern)
                        .collect::<Vec<_>>();
                    if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::at(
                            self.pattern_span(&arm.pattern),
                            "unreachable match arm",
                        ));
                    }

                    self.check_match_guard(
                        arm.guard.as_ref(),
                        &arm.pattern,
                        borrow_mode,
                        &mut arm_locals,
                    )?;
                    let arm_ty = self.type_of_match_arm_value(
                        &arm.value,
                        &mut arm_locals,
                        result_ty.as_ref(),
                        result_use,
                    )?;
                    if let Some(expected_ty) = result_ty.as_ref() {
                        if arm_ty != *expected_ty {
                            if capturing_closure_branch_mismatch(expected_ty, &arm_ty) {
                                return Err(capturing_closure_branch_diagnostic(
                                    "match",
                                    "arm",
                                    arm.value.span,
                                ));
                            }
                            return Err(Diagnostic::at(
                                arm.value.span,
                                format!(
                                    "match arm expression expects `{}`, found `{}`",
                                    expected_ty, arm_ty
                                ),
                            ));
                        }
                        result_ty = Some(merge_type_callable_contracts(expected_ty, &arm_ty));
                    } else {
                        result_ty = Some(arm_ty);
                    }
                    arm_states.push(arm_locals);
                }

                for (variant_name, payloads) in &variants {
                    if covered.contains(variant_name) {
                        continue;
                    }
                    let Some(patterns) = patterns_by_variant.get(variant_name) else {
                        continue;
                    };
                    let pattern_refs = patterns.iter().collect::<Vec<_>>();
                    if self.variant_patterns_cover_payloads_union(&pattern_refs, payloads) {
                        covered.insert(variant_name.clone());
                    }
                }

                let branch_states = arm_states.iter().collect::<Vec<_>>();
                self.merge_control_flow_moves(locals, &branch_states);

                let pattern_refs = arms
                    .iter()
                    .filter(|arm| arm.guard.is_none())
                    .map(|arm| &arm.pattern)
                    .collect::<Vec<_>>();
                let missing = self.missing_patterns_for_type(&pattern_refs, &scrutinee_ty);
                if !wildcard_seen && !missing.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "non-exhaustive match over `{}`: missing {}",
                            enum_name,
                            missing
                                .iter()
                                .map(|name| format!("`{}`", name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ));
                }

                return Ok(result_ty.unwrap_or(Type::Unit));
            }

            let class_scrutinee = match &scrutinee_ty {
                Type::Named(name, _) => self.resolve_class_info(name).is_some(),
                _ => false,
            };
            if class_scrutinee {
                if let Some(pattern) = arms
                    .iter()
                    .map(|arm| &arm.pattern)
                    .find(|pattern| pattern_contains_variant_shape(pattern))
                {
                    return Err(Diagnostic::coded_at(
                        "AU2999",
                        self.pattern_span(pattern),
                        "class patterns are not supported; match an explicit enum/tag representation or use a wildcard and ordinary code",
                    ));
                }
            }

            if !matches!(scrutinee_ty, Type::Tuple(_) | Type::Named(_, _))
                || !(matches!(scrutinee_ty, Type::Tuple(_))
                    || is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                    || is_string_type(&scrutinee_ty)
                    || class_scrutinee)
            {
                return Err(Diagnostic::at(
                span,
                format!(
                    "`match` currently requires a tuple, enum, bool, integer, float, or str scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
            }

            let mut wildcard_seen = false;
            let mut covered_literals = BTreeSet::<LiteralPatternKey>::new();
            let mut covered_bools = BTreeSet::<bool>::new();
            let mut arm_states = Vec::new();

            for (index, arm) in arms.iter().enumerate() {
                let mut arm_locals = locals.clone();
                if self.is_copy_type(&scrutinee_ty) {
                    if let Some(place) = shared_match_place.as_ref() {
                        self.retain_shared_match_place(place, span, &mut arm_locals);
                    }
                }
                match &arm.pattern {
                    Pattern::Or(pattern) => {
                        self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                    Pattern::Wildcard(wildcard_span) => {
                        if arm.guard.is_none() {
                            if index + 1 != arms.len() {
                                return Err(Diagnostic::at(
                                    *wildcard_span,
                                    "wildcard match arm must be the final `case`",
                                ));
                            }
                            wildcard_seen = true;
                        }
                    }
                    Pattern::Literal(pattern) => {
                        let key = self.literal_pattern_key(pattern, &scrutinee_ty)?;
                        if arm.guard.is_none() {
                            covered_literals.insert(key.clone());
                            if let LiteralPatternKey::Bool(value) = key {
                                covered_bools.insert(value);
                            }
                        }
                    }
                    Pattern::Variant(pattern) => {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "match over `{}` only supports literal patterns and `_`",
                                scrutinee_ty
                            ),
                        ));
                    }
                    Pattern::Binding(binding) => {
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                        if arm.guard.is_none() {
                            if index + 1 != arms.len() {
                                return Err(Diagnostic::at(
                                    binding.span,
                                    "catch-all match arm must be the final `case`",
                                ));
                            }
                            wildcard_seen = true;
                        }
                    }
                    Pattern::Tuple(tuple) => {
                        if !matches!(scrutinee_ty, Type::Tuple(_)) {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "tuple pattern requires a tuple scrutinee, found `{}`",
                                    scrutinee_ty
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                }

                let prior_patterns = arms[..index]
                    .iter()
                    .filter(|previous_arm| previous_arm.guard.is_none())
                    .map(|previous_arm| &previous_arm.pattern)
                    .collect::<Vec<_>>();
                if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                    return Err(Diagnostic::at(
                        self.pattern_span(&arm.pattern),
                        "unreachable match arm",
                    ));
                }

                self.check_match_guard(
                    arm.guard.as_ref(),
                    &arm.pattern,
                    borrow_mode,
                    &mut arm_locals,
                )?;
                let arm_ty = self.type_of_match_arm_value(
                    &arm.value,
                    &mut arm_locals,
                    result_ty.as_ref(),
                    result_use,
                )?;
                if let Some(expected_ty) = result_ty.as_ref() {
                    if arm_ty != *expected_ty {
                        if capturing_closure_branch_mismatch(expected_ty, &arm_ty) {
                            return Err(capturing_closure_branch_diagnostic(
                                "match",
                                "arm",
                                arm.value.span,
                            ));
                        }
                        return Err(Diagnostic::at(
                            arm.value.span,
                            format!(
                                "match arm expression expects `{}`, found `{}`",
                                expected_ty, arm_ty
                            ),
                        ));
                    }
                    result_ty = Some(merge_type_callable_contracts(expected_ty, &arm_ty));
                } else {
                    result_ty = Some(arm_ty);
                }
                arm_states.push(arm_locals);
            }

            if matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                && !wildcard_seen
                && covered_bools.len() < 2
            {
                let missing = [false, true]
                    .into_iter()
                    .filter(|value| !covered_bools.contains(value))
                    .map(|value| format!("`{}`", value))
                    .collect::<Vec<_>>();
                return Err(Diagnostic::at(
                    span,
                    format!("non-exhaustive bool match: missing {}", missing.join(", ")),
                ));
            }
            if !wildcard_seen
                && (is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || is_string_type(&scrutinee_ty))
            {
                return Err(Diagnostic::at(
                span,
                format!(
                    "match over `{}` requires a final wildcard arm because the domain is open-ended",
                    scrutinee_ty
                ),
                ));
            }
            if !wildcard_seen && matches!(scrutinee_ty, Type::Tuple(_)) {
                let patterns = arms
                    .iter()
                    .filter(|arm| arm.guard.is_none())
                    .map(|arm| &arm.pattern)
                    .collect::<Vec<_>>();
                if !self
                    .missing_patterns_for_type(&patterns, &scrutinee_ty)
                    .is_empty()
                {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "non-exhaustive match over `{}`: add a covering tuple pattern or final `case _:`",
                            scrutinee_ty
                        ),
                    ));
                }
            }

            let branch_states = arm_states.iter().collect::<Vec<_>>();
            self.merge_control_flow_moves(locals, &branch_states);

            Ok(result_ty.unwrap_or(Type::Unit))
        })();
        self.end_match_borrow_mut(active_match_borrow);
        result
    }

    fn render_literal_pattern(&self, pattern: &LiteralPattern) -> String {
        match &pattern.kind {
            LiteralPatternKind::Int(value) => value.to_string(),
            LiteralPatternKind::Float(value) => value.to_string(),
            LiteralPatternKind::Bool(value) => value.to_string(),
            LiteralPatternKind::String(value) => format!("{:?}", value),
        }
    }

    fn literal_pattern_key(
        &self,
        pattern: &LiteralPattern,
        scrutinee_ty: &Type,
    ) -> Result<LiteralPatternKey> {
        match &pattern.kind {
            LiteralPatternKind::Int(value) => {
                let Some(bounds) = integer_type_bounds(scrutinee_ty) else {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                };
                if !value.fits_bounds(bounds) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not fit scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Int(*value))
            }
            LiteralPatternKind::Float(value) => {
                if !is_float_type(scrutinee_ty) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Float(value.to_bits()))
            }
            LiteralPatternKind::Bool(value) => {
                if !matches!(scrutinee_ty, Type::Named(name, args) if name == "bool" && args.is_empty())
                {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Bool(*value))
            }
            LiteralPatternKind::String(value) => {
                if !is_string_type(scrutinee_ty) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern {:?} does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::String(value.clone()))
            }
        }
    }

    fn resolve_member_type(
        &self,
        object_ty: &Type,
        field: &str,
        span: crate::diag::Span,
    ) -> Result<Type> {
        let (name, args) = match object_ty {
            Type::Union(_) => {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot access field `{field}` on union `{object_ty}` without narrowing"
                    ),
                ))
            }
            Type::Module(path) => {
                let namespace = self.module_namespace(path).ok_or_else(|| {
                    Diagnostic::at(span, format!("unknown module namespace `{}`", path))
                })?;
                if let Some(child) = namespace.modules.get(field) {
                    return Ok(Type::Module(child.path.clone()));
                }
                if let Some(constant) = namespace.constants.get(field) {
                    return Ok(constant.ty.clone());
                }
                if namespace.functions.contains_key(field) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "function `{}` from module `{}` must be called with `(...)`",
                            field, path
                        ),
                    ));
                }
                if namespace.extern_functions.contains_key(field) {
                    return Err(Diagnostic::coded_at(
                        "AU2999",
                        span,
                        format!(
                            "extern function `{}` from module `{}` is direct-call-only",
                            field, path
                        ),
                    )
                    .with_help(format!("call `{path}.{field}(...)` synchronously")));
                }
                if namespace.classes.contains_key(field) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` from module `{}` must be constructed with `(...)`",
                            field, path
                        ),
                    ));
                }
                if namespace.opaque_handles.contains_key(field) {
                    return Err(Diagnostic::coded_at(
                        "AU2005",
                        span,
                        format!(
                            "opaque FFI handle type `{}` from module `{}` is not a value",
                            field, path
                        ),
                    ));
                }
                if let Some(enum_info) = namespace.enums.get(field) {
                    return Ok(Type::Named(
                        self.module_enum_type_name(path, enum_info),
                        Vec::new(),
                    ));
                }
                return Err(Diagnostic::at(
                    span,
                    format!("module `{}` has no member `{}`", path, field),
                ));
            }
            Type::Named(name, args) => (name, args),
            Type::TypeParam(type_param_name) => {
                return self
                    .trait_method_from_type_param(type_param_name, field)
                    .map(|method| method.signature.return_type.clone())
                    .map_err(|_| {
                        Diagnostic::at(
                            span,
                            format!("cannot access field `{}` on `{}`", field, object_ty),
                        )
                    });
            }
            Type::Function { .. } | Type::Closure { .. } | Type::Tuple(_) | Type::Unit => {
                return Err(Diagnostic::at(
                    span,
                    format!("cannot access field `{}` on `{}`", field, object_ty),
                ));
            }
        };

        if let Some(variant_payloads) = self.builtin_enum_variant_payload(object_ty, name, field) {
            return match variant_payloads.is_empty() {
                false => Err(Diagnostic::at(
                    span,
                    format!("variant `{}` of enum `{}` requires a payload", field, name),
                )),
                true => Ok(object_ty.clone()),
            };
        }

        if BuiltinMember::resolve(name, field).is_some() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "method `{}` on `{}` must be called with `(...)`",
                    field, object_ty
                ),
            ));
        }

        let Some(class_info) = self.resolve_class_info(name) else {
            return Err(Diagnostic::at(
                span,
                format!("type `{}` has no field `{}`", name, field),
            ));
        };
        let substitutions = substitutions_from_decl_type_args(&class_info.decl.type_params, args);
        if let Some(field_info) = class_info.fields.get(field) {
            if self.is_external_module(&class_info.module_name) && !field_info.public {
                return Err(Diagnostic::at(
                    span,
                    format!("field `{}` is private on `{}`", field, class_info.decl.name),
                ));
            }
            let field_ty = substitute_type(&field_info.ty, &substitutions);
            return Ok(erase_type_callable_contracts(&field_ty));
        }
        if let Some(method) = class_info.methods.get(field) {
            if self.is_external_module(&class_info.module_name) && !method.decl.public {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "method `{}` is private on `{}`",
                        field, class_info.decl.name
                    ),
                ));
            }
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "method values are not supported in this language version; call `.{field}(...)` directly or wrap it in a named function"
                ),
            ));
        }
        if self
            .trait_method_for_concrete_type(object_ty, field, span)?
            .is_some()
        {
            return Err(Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "trait-dispatched method values are not supported in this language version; call `.{field}(...)` directly or wrap it in a named function"
                ),
            ));
        }
        Err(Diagnostic::at(
            span,
            format!("class `{}` has no field `{}`", name, field),
        ))
    }

    fn resolve_member_target_type(
        &self,
        object: &Expr,
        field: &str,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let object_ty = self.type_of_member_object_expr(object, locals)?;
        self.resolve_member_type(&object_ty, field, span)
    }

    fn builtin_payload_free_variant(enum_name: &str, variant_name: &str) -> bool {
        matches!(
            (enum_name, variant_name),
            ("Option", "None")
                | ("QueueReceive", "Closed" | "TimedOut" | "Cancelled")
                | ("TaskResult", "TimedOut" | "Cancelled")
                | ("WaitAny", "TimedOut" | "Cancelled")
                | ("WaitAll", "TimedOut" | "Cancelled")
                | ("SelectOutcome", "Cancelled")
        )
    }

    fn is_payload_free_variant_expr(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Group(inner) | ExprKind::Specialize { expr: inner, .. } => {
                self.is_payload_free_variant_expr(inner)
            }
            ExprKind::Member { object, field } => {
                let base_object = match &object.kind {
                    ExprKind::Specialize { expr, .. } => &**expr,
                    _ => &**object,
                };
                if let ExprKind::Name(enum_name) = &base_object.kind {
                    if Self::builtin_payload_free_variant(enum_name, field) {
                        return true;
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        return enum_info
                            .variants
                            .get(field)
                            .is_some_and(|variant| variant.payloads.is_empty());
                    }
                }
                if let Some((module_path, enum_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        return namespace
                            .enums
                            .get(&enum_name)
                            .and_then(|enum_info| enum_info.variants.get(field))
                            .is_some_and(|variant| variant.payloads.is_empty());
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn is_enum_constructor_object(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Group(inner) | ExprKind::Specialize { expr: inner, .. } => {
                self.is_enum_constructor_object(inner)
            }
            ExprKind::Name(name) => {
                matches!(
                    name.as_str(),
                    "Option"
                        | "Result"
                        | "SendError"
                        | "QueueReceive"
                        | "TaskResult"
                        | "WaitAny"
                        | "WaitAll"
                        | "SelectOutcome"
                ) || self.resolve_enum_info(name).is_some()
            }
            _ => self
                .qualified_module_item(expr)
                .and_then(|(module_path, enum_name)| {
                    self.module_namespace(&module_path)
                        .and_then(|namespace| namespace.enums.get(&enum_name))
                })
                .is_some(),
        }
    }

    fn newly_moved_places(
        &self,
        before: &HashMap<String, LocalBinding>,
        after: &HashMap<String, LocalBinding>,
    ) -> Vec<PlacePath> {
        let mut moved = Vec::new();
        for (name, current) in after {
            let previous = before.get(name);
            let previously_moved = previous.map(|binding| binding.moved).unwrap_or(false);
            if current.moved && !previously_moved {
                moved.push(PlacePath::root(name.clone()));
            }
            let previous_fields = previous
                .map(|binding| binding.moved_fields.clone())
                .unwrap_or_default();
            for field in current
                .moved_fields
                .keys()
                .filter(|field| !previous_fields.contains_key(*field))
            {
                moved.push(PlacePath {
                    root: name.clone(),
                    projections: field.clone(),
                });
            }
        }
        moved
    }

    fn newly_moved_place_accesses(
        &self,
        before: &HashMap<String, LocalBinding>,
        after: &HashMap<String, LocalBinding>,
        label: &str,
        fallback_span: crate::diag::Span,
    ) -> Vec<BorrowedCallPlace> {
        self.newly_moved_places(before, after)
            .into_iter()
            .map(|path| {
                let origin_span = after
                    .get(&path.root)
                    .and_then(|binding| {
                        if path.is_root() {
                            binding.moved_at
                        } else {
                            binding.moved_fields.get(&path.projections).copied()
                        }
                    })
                    .unwrap_or(fallback_span);
                BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Value,
                    param_name: label.to_string(),
                    origin_span,
                }
            })
            .collect()
    }

    fn render_variant_pattern_shape(&self, variant_name: &str, payload_tys: &[Type]) -> String {
        if payload_tys.is_empty() {
            return variant_name.to_string();
        }
        let payload = std::iter::repeat_n("_", payload_tys.len())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{variant_name}({payload})")
    }

    fn missing_patterns_for_type(&self, patterns: &[&Pattern], expected_ty: &Type) -> Vec<String> {
        if patterns
            .iter()
            .any(|pattern| self.pattern_covers_entire_type(pattern, expected_ty))
        {
            return Vec::new();
        }
        if let Type::Tuple(element_types) = expected_ty {
            if self.tuple_patterns_cover_type_union(patterns, element_types) {
                return Vec::new();
            }
            return vec!["_".to_string()];
        }
        let Some(variants) = self.enum_variants_for_type(expected_ty) else {
            return vec!["_".to_string()];
        };
        let mut grouped = BTreeMap::<String, Vec<&VariantPattern>>::new();
        for pattern in patterns {
            let mut pending = vec![*pattern];
            while let Some(pattern) = pending.pop() {
                match pattern {
                    Pattern::Or(or_pattern) => pending.extend(or_pattern.alternatives.iter()),
                    Pattern::Variant(variant_pattern) => {
                        grouped
                            .entry(variant_pattern.variant_name.clone())
                            .or_default()
                            .push(variant_pattern);
                    }
                    _ => {}
                }
            }
        }
        let mut missing = Vec::new();
        for (variant_name, payload_tys) in variants {
            let variant_patterns = grouped.get(&variant_name).cloned().unwrap_or_default();
            if variant_patterns.is_empty() {
                missing.push(self.render_variant_pattern_shape(&variant_name, &payload_tys));
                continue;
            }
            if self.variant_patterns_cover_payloads_union(&variant_patterns, &payload_tys) {
                continue;
            }
            if payload_tys.len() == 1 {
                let nested_patterns = variant_patterns
                    .iter()
                    .filter_map(|pattern| pattern.subpatterns.first())
                    .collect::<Vec<_>>();
                let nested_missing =
                    self.missing_patterns_for_type(&nested_patterns, &payload_tys[0]);
                if !nested_missing.is_empty() {
                    missing.extend(
                        nested_missing
                            .into_iter()
                            .map(|nested| format!("{variant_name}({nested})")),
                    );
                    continue;
                }
            }
            missing.push(self.render_variant_pattern_shape(&variant_name, &payload_tys));
        }
        missing
    }

    fn pattern_span(&self, pattern: &Pattern) -> crate::diag::Span {
        match pattern {
            Pattern::Or(pattern) => pattern.span,
            Pattern::Wildcard(span) => *span,
            Pattern::Literal(pattern) => pattern.span,
            Pattern::Binding(binding) => binding.span,
            Pattern::Variant(variant) => variant.span,
            Pattern::Tuple(tuple) => tuple.span,
        }
    }

    fn check_match_guard(
        &self,
        guard: Option<&Expr>,
        pattern: &Pattern,
        borrow_mode: ReceiverKind,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let Some(guard) = guard else {
            return Ok(());
        };
        let mut candidate_locals;
        let guard_locals = if borrow_mode == ReceiverKind::Value {
            candidate_locals = locals.clone();
            let mut names = BTreeSet::new();
            Self::collect_pattern_binding_names(pattern, &mut names);
            for name in names {
                if let Some(binding) = candidate_locals.get_mut(&name) {
                    if !self.is_copy_type(&binding.ty) {
                        binding.passing = ReceiverKind::Borrow;
                        binding.borrowed_at = Some(guard.span);
                    }
                }
            }
            &mut candidate_locals
        } else {
            locals
        };
        self.reject_mutable_returned_view_value(guard, guard_locals, false)?;
        let actual = match self.type_of_expr(guard, guard_locals) {
            Ok(actual) => actual,
            Err(mut diagnostic)
                if borrow_mode == ReceiverKind::Value
                    && diagnostic.code == "AU3002"
                    && diagnostic.message.starts_with("cannot move borrowed value") =>
            {
                diagnostic.code = "AU3001".to_string();
                diagnostic.message =
                    "cannot move an owned match candidate before its guard commits the arm"
                        .to_string();
                return Err(diagnostic);
            }
            Err(diagnostic) => return Err(diagnostic),
        };
        let expected = Type::named("bool");
        if actual != expected {
            return Err(Diagnostic::coded_at(
                "AU2002",
                guard.span,
                format!("match guard expects exactly `bool`, found `{actual}`"),
            ));
        }
        Ok(())
    }

    fn validate_or_pattern_alternatives(
        &self,
        pattern: &crate::ast::OrPattern,
        expected_ty: &Type,
    ) -> Result<()> {
        let mut prior = Vec::<&Pattern>::new();
        for alternative in &pattern.alternatives {
            if self.patterns_cover_pattern(&prior, alternative, expected_ty) {
                return Err(Diagnostic::coded_at(
                    "AU2999",
                    self.pattern_span(alternative),
                    "duplicate or subsumed alternative in or-pattern",
                ));
            }
            prior.push(alternative);
        }
        Ok(())
    }

    fn patterns_cover_pattern(
        &self,
        patterns: &[&Pattern],
        pattern: &Pattern,
        expected_ty: &Type,
    ) -> bool {
        match pattern {
            Pattern::Or(or_pattern) => or_pattern
                .alternatives
                .iter()
                .all(|alternative| self.patterns_cover_pattern(patterns, alternative, expected_ty)),
            Pattern::Wildcard(_) | Pattern::Binding(_) => {
                if self.patterns_cover_type_union(patterns, expected_ty) {
                    return true;
                }
                if matches!(expected_ty, Type::Named(name, args) if name == "bool" && args.is_empty())
                {
                    let mut covered = BTreeSet::new();
                    for previous in patterns {
                        match previous {
                            Pattern::Or(or_pattern) => {
                                for alternative in &or_pattern.alternatives {
                                    if let Pattern::Literal(literal) = alternative {
                                        if let Ok(LiteralPatternKey::Bool(value)) =
                                            self.literal_pattern_key(literal, expected_ty)
                                        {
                                            covered.insert(value);
                                        }
                                    }
                                }
                            }
                            Pattern::Wildcard(_) | Pattern::Binding(_) => return true,
                            Pattern::Literal(literal) => {
                                if let Ok(LiteralPatternKey::Bool(value)) =
                                    self.literal_pattern_key(literal, expected_ty)
                                {
                                    covered.insert(value);
                                }
                            }
                            Pattern::Variant(_) => {}
                            Pattern::Tuple(_) => {}
                        }
                    }
                    return covered.len() == 2;
                }
                patterns
                    .iter()
                    .any(|previous| self.pattern_covers_entire_type(previous, expected_ty))
            }
            Pattern::Variant(current_variant) => {
                if patterns.iter().any(|previous| {
                    self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)
                }) {
                    return true;
                }
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payload_tys)) = variants
                    .iter()
                    .find(|(variant_name, _)| variant_name == &current_variant.variant_name)
                else {
                    return false;
                };
                let variant_patterns = patterns
                    .iter()
                    .filter_map(|previous| match previous {
                        Pattern::Variant(variant)
                            if variant.variant_name == current_variant.variant_name =>
                        {
                            Some(variant)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if current_variant.subpatterns.len() != payload_tys.len() {
                    return false;
                }
                let rows = variant_patterns
                    .iter()
                    .map(|variant| variant.subpatterns.clone())
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_pattern_union(
                    &rows,
                    &current_variant.subpatterns,
                    payload_tys,
                )
            }
            Pattern::Tuple(current_tuple) => {
                if patterns.iter().any(|previous| {
                    self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)
                }) {
                    return true;
                }
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                if current_tuple.elements.len() != element_types.len() {
                    return false;
                }
                let rows = patterns
                    .iter()
                    .filter_map(|previous| match previous {
                        Pattern::Tuple(tuple) if tuple.elements.len() == element_types.len() => {
                            Some(tuple.elements.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_pattern_union(&rows, &current_tuple.elements, element_types)
            }
            Pattern::Literal(_) => patterns
                .iter()
                .any(|previous| self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)),
        }
    }

    fn pattern_is_covered_by_pattern(
        &self,
        previous: &Pattern,
        current: &Pattern,
        expected_ty: &Type,
    ) -> bool {
        if self.pattern_covers_entire_type(previous, expected_ty) {
            return true;
        }
        match (previous, current) {
            (Pattern::Or(previous), _) => previous.alternatives.iter().any(|alternative| {
                self.pattern_is_covered_by_pattern(alternative, current, expected_ty)
            }),
            (_, Pattern::Or(current)) => current.alternatives.iter().all(|alternative| {
                self.pattern_is_covered_by_pattern(previous, alternative, expected_ty)
            }),
            (Pattern::Literal(previous), Pattern::Literal(current)) => {
                self.literal_pattern_key(previous, expected_ty).ok()
                    == self.literal_pattern_key(current, expected_ty).ok()
            }
            (Pattern::Variant(previous), Pattern::Variant(current)) => {
                if previous.variant_name != current.variant_name {
                    return false;
                }
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payload_tys)) = variants
                    .iter()
                    .find(|(variant_name, _)| variant_name == &current.variant_name)
                else {
                    return false;
                };
                if previous.subpatterns.len() != payload_tys.len()
                    || current.subpatterns.len() != payload_tys.len()
                {
                    return false;
                }
                previous
                    .subpatterns
                    .iter()
                    .zip(current.subpatterns.iter())
                    .zip(payload_tys.iter())
                    .all(|((previous, current), payload_ty)| {
                        self.pattern_is_covered_by_pattern(previous, current, payload_ty)
                    })
            }
            (Pattern::Tuple(previous), Pattern::Tuple(current)) => {
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                previous.elements.len() == element_types.len()
                    && current.elements.len() == element_types.len()
                    && previous
                        .elements
                        .iter()
                        .zip(&current.elements)
                        .zip(element_types)
                        .all(|((previous, current), ty)| {
                            self.pattern_is_covered_by_pattern(previous, current, ty)
                        })
            }
            _ => false,
        }
    }

    fn pattern_covers_entire_type(&self, pattern: &Pattern, expected_ty: &Type) -> bool {
        match pattern {
            Pattern::Or(pattern) => {
                let alternatives = pattern.alternatives.iter().collect::<Vec<_>>();
                self.patterns_cover_type_union(&alternatives, expected_ty)
            }
            Pattern::Wildcard(_) | Pattern::Binding(_) => true,
            Pattern::Literal(_) => false,
            Pattern::Tuple(tuple) => {
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                tuple.elements.len() == element_types.len()
                    && tuple
                        .elements
                        .iter()
                        .zip(element_types)
                        .all(|(element, ty)| self.pattern_covers_entire_type(element, ty))
            }
            Pattern::Variant(variant_pattern) => {
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payloads)) = variants
                    .iter()
                    .find(|(name, _)| name == &variant_pattern.variant_name)
                else {
                    return false;
                };
                if variants.len() != 1 || payloads.len() != variant_pattern.subpatterns.len() {
                    return false;
                }
                variant_pattern.subpatterns.iter().zip(payloads.iter()).all(
                    |(subpattern, payload_ty)| {
                        self.pattern_covers_entire_type(subpattern, payload_ty)
                    },
                )
            }
        }
    }

    fn variant_pattern_covers_payloads(
        &self,
        variant_pattern: &VariantPattern,
        payload_tys: &[Type],
    ) -> bool {
        variant_pattern.subpatterns.len() == payload_tys.len()
            && variant_pattern
                .subpatterns
                .iter()
                .zip(payload_tys.iter())
                .all(|(subpattern, payload_ty)| {
                    self.pattern_covers_entire_type(subpattern, payload_ty)
                })
    }

    fn patterns_cover_type_union(&self, patterns: &[&Pattern], expected_ty: &Type) -> bool {
        if patterns
            .iter()
            .any(|pattern| self.pattern_covers_entire_type(pattern, expected_ty))
        {
            return true;
        }
        if let Type::Tuple(element_types) = expected_ty {
            return self.tuple_patterns_cover_type_union(patterns, element_types);
        }
        let Some(variants) = self.enum_variants_for_type(expected_ty) else {
            return false;
        };
        let mut grouped = BTreeMap::<String, Vec<&VariantPattern>>::new();
        for pattern in patterns {
            let mut pending = vec![*pattern];
            while let Some(pattern) = pending.pop() {
                match pattern {
                    Pattern::Or(or_pattern) => pending.extend(or_pattern.alternatives.iter()),
                    Pattern::Variant(variant_pattern) => {
                        grouped
                            .entry(variant_pattern.variant_name.clone())
                            .or_default()
                            .push(variant_pattern);
                    }
                    _ => {}
                }
            }
        }
        variants.into_iter().all(|(variant_name, payloads)| {
            let Some(variant_patterns) = grouped.get(&variant_name) else {
                return false;
            };
            self.variant_patterns_cover_payloads_union(variant_patterns, &payloads)
        })
    }

    fn tuple_patterns_cover_type_union(
        &self,
        patterns: &[&Pattern],
        element_types: &[Type],
    ) -> bool {
        let rows = patterns
            .iter()
            .flat_map(|pattern| {
                let alternatives: Vec<&Pattern> = match pattern {
                    Pattern::Or(or_pattern) => or_pattern.alternatives.iter().collect(),
                    pattern => vec![*pattern],
                };
                alternatives
                    .into_iter()
                    .filter_map(|pattern| match pattern {
                        Pattern::Tuple(tuple) if tuple.elements.len() == element_types.len() => {
                            Some(tuple.elements.clone())
                        }
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&rows, element_types)
    }

    fn pattern_rows_cover_type_union(&self, rows: &[Vec<Pattern>], types: &[Type]) -> bool {
        let Some((first_ty, remaining_types)) = types.split_first() else {
            return !rows.is_empty();
        };
        let irrefutable =
            |pattern: &Pattern| matches!(pattern, Pattern::Wildcard(_) | Pattern::Binding(_));

        if matches!(first_ty, Type::Named(name, args) if name == "bool" && args.is_empty()) {
            return [false, true].into_iter().all(|expected| {
                let specialized = rows
                    .iter()
                    .filter_map(|row| {
                        let (first, remaining) = row.split_first()?;
                        if irrefutable(first)
                            || matches!(
                                first,
                                Pattern::Literal(LiteralPattern {
                                    kind: LiteralPatternKind::Bool(actual),
                                    ..
                                }) if *actual == expected
                            )
                        {
                            Some(remaining.to_vec())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_type_union(&specialized, remaining_types)
            });
        }

        if let Type::Tuple(nested_types) = first_ty {
            let mut specialized_types = nested_types.clone();
            specialized_types.extend_from_slice(remaining_types);
            let specialized = rows
                .iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    let mut values = match first {
                        pattern if irrefutable(pattern) => {
                            vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                        }
                        Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                            tuple.elements.clone()
                        }
                        _ => return None,
                    };
                    values.extend_from_slice(remaining);
                    Some(values)
                })
                .collect::<Vec<_>>();
            return self.pattern_rows_cover_type_union(&specialized, &specialized_types);
        }

        if let Some(variants) = self.enum_variants_for_type(first_ty) {
            return variants.into_iter().all(|(variant_name, payload_types)| {
                let mut specialized_types = payload_types.clone();
                specialized_types.extend_from_slice(remaining_types);
                let specialized = rows
                    .iter()
                    .filter_map(|row| {
                        let (first, remaining) = row.split_first()?;
                        let mut values = match first {
                            pattern if irrefutable(pattern) => {
                                vec![
                                    Pattern::Wildcard(self.pattern_span(pattern));
                                    payload_types.len()
                                ]
                            }
                            Pattern::Variant(variant)
                                if variant.variant_name == variant_name
                                    && variant.subpatterns.len() == payload_types.len() =>
                            {
                                variant.subpatterns.clone()
                            }
                            _ => return None,
                        };
                        values.extend_from_slice(remaining);
                        Some(values)
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_type_union(&specialized, &specialized_types)
            });
        }

        let specialized = rows
            .iter()
            .filter_map(|row| {
                let (first, remaining) = row.split_first()?;
                irrefutable(first).then(|| remaining.to_vec())
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&specialized, remaining_types)
    }

    fn pattern_rows_cover_pattern_union(
        &self,
        rows: &[Vec<Pattern>],
        current: &[Pattern],
        types: &[Type],
    ) -> bool {
        let Some((first_ty, remaining_types)) = types.split_first() else {
            return current.is_empty() && !rows.is_empty();
        };
        let Some((current_first, current_remaining)) = current.split_first() else {
            return false;
        };
        let irrefutable =
            |pattern: &Pattern| matches!(pattern, Pattern::Wildcard(_) | Pattern::Binding(_));

        let specialize_bool = |expected: bool| {
            rows.iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    if irrefutable(first)
                        || matches!(
                            first,
                            Pattern::Literal(LiteralPattern {
                                kind: LiteralPatternKind::Bool(actual),
                                ..
                            }) if *actual == expected
                        )
                    {
                        Some(remaining.to_vec())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        if matches!(first_ty, Type::Named(name, args) if name == "bool" && args.is_empty()) {
            return match current_first {
                Pattern::Wildcard(_) | Pattern::Binding(_) => {
                    [false, true].into_iter().all(|expected| {
                        self.pattern_rows_cover_pattern_union(
                            &specialize_bool(expected),
                            current_remaining,
                            remaining_types,
                        )
                    })
                }
                Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Bool(expected),
                    ..
                }) => self.pattern_rows_cover_pattern_union(
                    &specialize_bool(*expected),
                    current_remaining,
                    remaining_types,
                ),
                _ => false,
            };
        }

        if let Type::Tuple(nested_types) = first_ty {
            let current_nested = match current_first {
                pattern if irrefutable(pattern) => {
                    vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                }
                Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                    tuple.elements.clone()
                }
                _ => return false,
            };
            let mut specialized_types = nested_types.clone();
            specialized_types.extend_from_slice(remaining_types);
            let mut specialized_current = current_nested;
            specialized_current.extend_from_slice(current_remaining);
            let specialized_rows = rows
                .iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    let mut values = match first {
                        pattern if irrefutable(pattern) => {
                            vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                        }
                        Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                            tuple.elements.clone()
                        }
                        _ => return None,
                    };
                    values.extend_from_slice(remaining);
                    Some(values)
                })
                .collect::<Vec<_>>();
            return self.pattern_rows_cover_pattern_union(
                &specialized_rows,
                &specialized_current,
                &specialized_types,
            );
        }

        if let Some(variants) = self.enum_variants_for_type(first_ty) {
            let current_variant_payloads = match current_first {
                pattern if irrefutable(pattern) => None,
                Pattern::Variant(current_variant) => Some(current_variant.subpatterns.clone()),
                _ => return false,
            };
            let variants_to_cover = if let Pattern::Variant(current_variant) = current_first {
                variants
                    .into_iter()
                    .filter(|(variant_name, payload_types)| {
                        variant_name == &current_variant.variant_name
                            && payload_types.len() == current_variant.subpatterns.len()
                    })
                    .collect()
            } else {
                variants
            };
            if variants_to_cover.is_empty() {
                return false;
            }
            return variants_to_cover
                .into_iter()
                .all(|(variant_name, payload_types)| {
                    let current_payloads = current_variant_payloads.clone().unwrap_or_else(|| {
                        vec![
                            Pattern::Wildcard(self.pattern_span(current_first));
                            payload_types.len()
                        ]
                    });
                    let mut specialized_types = payload_types.clone();
                    specialized_types.extend_from_slice(remaining_types);
                    let mut specialized_current = current_payloads;
                    specialized_current.extend_from_slice(current_remaining);
                    let specialized_rows = rows
                        .iter()
                        .filter_map(|row| {
                            let (first, remaining) = row.split_first()?;
                            let mut values = match first {
                                pattern if irrefutable(pattern) => {
                                    vec![
                                        Pattern::Wildcard(self.pattern_span(pattern));
                                        payload_types.len()
                                    ]
                                }
                                Pattern::Variant(variant)
                                    if variant.variant_name == variant_name
                                        && variant.subpatterns.len() == payload_types.len() =>
                                {
                                    variant.subpatterns.clone()
                                }
                                _ => return None,
                            };
                            values.extend_from_slice(remaining);
                            Some(values)
                        })
                        .collect::<Vec<_>>();
                    self.pattern_rows_cover_pattern_union(
                        &specialized_rows,
                        &specialized_current,
                        &specialized_types,
                    )
                });
        }

        let specialized = rows
            .iter()
            .filter_map(|row| {
                let (first, remaining) = row.split_first()?;
                match current_first {
                    Pattern::Wildcard(_) | Pattern::Binding(_) => {
                        irrefutable(first).then(|| remaining.to_vec())
                    }
                    Pattern::Literal(_) => self
                        .pattern_is_covered_by_pattern(first, current_first, first_ty)
                        .then(|| remaining.to_vec()),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_pattern_union(&specialized, current_remaining, remaining_types)
    }

    fn variant_patterns_cover_payloads_union(
        &self,
        patterns: &[&VariantPattern],
        payload_tys: &[Type],
    ) -> bool {
        if patterns
            .iter()
            .any(|pattern| self.variant_pattern_covers_payloads(pattern, payload_tys))
        {
            return true;
        }
        let rows = patterns
            .iter()
            .filter(|pattern| pattern.subpatterns.len() == payload_tys.len())
            .map(|pattern| pattern.subpatterns.clone())
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&rows, payload_tys)
    }

    fn field_path_is_moved(binding: &LocalBinding, path: &ProjectionPath) -> bool {
        binding
            .moved_fields
            .keys()
            .any(|moved_path| moved_path.overlaps(path))
    }

    fn moved_field_origin(
        binding: &LocalBinding,
        path: &ProjectionPath,
    ) -> Option<crate::diag::Span> {
        binding
            .moved_fields
            .iter()
            .find(|(moved_path, _)| moved_path.overlaps(path))
            .map(|(_, span)| *span)
    }

    fn clear_moved_field_path(binding: &mut LocalBinding, path: &ProjectionPath) {
        binding
            .moved_fields
            .retain(|moved_path, _| !moved_path.is_descendant_of_or_equal(path));
    }

    fn type_of_member_object_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        match &expr.kind {
            ExprKind::Name(name) => {
                let binding = locals
                    .get(name)
                    .ok_or_else(|| Diagnostic::at(expr.span, format!("unknown name `{}`", name)))?;
                self.ensure_pattern_binding_not_stale(name, expr.span, binding)?;
                if binding.moved {
                    return Err(self.moved_value_diagnostic(name, expr.span, binding));
                }
                Ok(binding.ty.clone())
            }
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => {
                self.type_of_member_object_expr(inner, locals)
            }
            ExprKind::Member { object, field } => {
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                self.resolve_member_type(&object_ty, field, expr.span)
            }
            ExprKind::Index { object, index } => {
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                if let Type::Tuple(elements) = object_ty {
                    let ExprKind::Int(value) = index.kind else {
                        return self.type_of_expr(expr, locals);
                    };
                    let index = usize::try_from(value).map_err(|_| {
                        Diagnostic::coded_at(
                            "AU3004",
                            index.span,
                            "invalid tuple projection position",
                        )
                    })?;
                    elements.get(index).cloned().ok_or_else(|| {
                        Diagnostic::coded_at(
                            "AU3004",
                            expr.span,
                            format!("tuple has no position {index}"),
                        )
                    })
                } else {
                    self.type_of_expr(expr, locals)
                }
            }
            _ => self.type_of_expr(expr, locals),
        }
    }

    fn enum_variants_for_type(&self, ty: &Type) -> Option<Vec<(String, Vec<Type>)>> {
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
            Type::Named(name, args) => self.resolve_enum_info(name).map(|enum_info| {
                let substitutions =
                    substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                enum_info
                    .decl
                    .variants
                    .iter()
                    .map(|variant| {
                        (
                            variant.name.clone(),
                            enum_info
                                .variants
                                .get(&variant.name)
                                .map(|info| {
                                    info.payloads
                                        .iter()
                                        .map(|payload| substitute_type(&payload.ty, &substitutions))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default(),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
            _ => None,
        }
    }

    fn builtin_enum_variant_payload(
        &self,
        expected: &Type,
        enum_name: &str,
        variant_name: &str,
    ) -> Option<Vec<Type>> {
        let Type::Named(expected_name, args) = expected else {
            return None;
        };
        if expected_name != enum_name {
            return None;
        }
        match (enum_name, variant_name, args.as_slice()) {
            ("Option", "Some", [inner]) => Some(vec![inner.clone()]),
            ("Option", "None", [_]) => Some(Vec::new()),
            ("Result", "Ok", [ok, _err]) => Some(vec![ok.clone()]),
            ("Result", "Err", [_ok, err]) => Some(vec![err.clone()]),
            ("SendError", "Closed" | "Cancelled" | "TimedOut" | "Full", [value]) => {
                Some(vec![value.clone()])
            }
            ("QueueReceive", "Item", [value]) => Some(vec![value.clone()]),
            ("QueueReceive", "Closed" | "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
            ("TaskResult", "Ready", [value]) => Some(vec![value.clone()]),
            ("TaskResult", "Error", [_]) => Some(vec![Type::named("str")]),
            ("TaskResult", "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
            ("WaitAny", "Ready", [value]) => Some(vec![Type::named("int64"), value.clone()]),
            ("WaitAny", "Error", [_]) => Some(vec![Type::named("int64"), Type::named("str")]),
            ("WaitAny", "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
            ("WaitAll", "Ready", [values]) => Some(vec![values.clone()]),
            ("WaitAll", "Error", [_]) => Some(vec![Type::named("int64"), Type::named("str")]),
            ("WaitAll", "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
            ("SelectOutcome", "Queue", [queue, _]) => Some(vec![
                Type::named("int64"),
                Type::Named("QueueReceive".to_string(), vec![queue.clone()]),
            ]),
            ("SelectOutcome", "Task", [_, task]) => Some(vec![
                Type::named("int64"),
                Type::Named("TaskResult".to_string(), vec![task.clone()]),
            ]),
            ("SelectOutcome", "Deadline", [_, _]) => Some(vec![Type::named("int64")]),
            ("SelectOutcome", "Cancelled", [_, _]) => Some(Vec::new()),
            _ => None,
        }
    }

    fn explicit_builtin_type(
        &self,
        name: &str,
        explicit_args: &[Type],
        span: crate::diag::Span,
    ) -> Result<Type> {
        let expected_len = match name {
            "Option" => 1,
            "Result" => 2,
            "SendError" => 1,
            "QueueReceive" => 1,
            "TaskResult" => 1,
            "WaitAny" => 1,
            "WaitAll" => 1,
            "SelectOutcome" => 2,
            _ => return Err(Diagnostic::at(span, format!("unknown name `{}`", name))),
        };
        if explicit_args.len() != expected_len {
            return Err(Diagnostic::at(
                span,
                format!(
                    "enum `{}` expects {} type argument{}, found {}",
                    name,
                    expected_len,
                    if expected_len == 1 { "" } else { "s" },
                    explicit_args.len()
                ),
            ));
        }
        Ok(Type::Named(name.to_string(), explicit_args.to_vec()))
    }

    fn expr_can_use_partial_expected_hint(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member { object, .. } => self.is_builtin_enum_constructor_expr(object),
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Member { object, .. } => self.is_builtin_enum_constructor_expr(object),
                _ => false,
            },
            ExprKind::Group(inner) => self.expr_can_use_partial_expected_hint(inner),
            _ => false,
        }
    }

    fn is_builtin_enum_constructor_expr(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Name(name) => matches!(
                name.as_str(),
                "Option"
                    | "Result"
                    | "SendError"
                    | "QueueReceive"
                    | "TaskResult"
                    | "WaitAny"
                    | "WaitAll"
                    | "SelectOutcome"
            ),
            ExprKind::Specialize { expr, .. } => self.is_builtin_enum_constructor_expr(expr),
            ExprKind::Group(inner) => self.is_builtin_enum_constructor_expr(inner),
            _ => false,
        }
    }

    fn is_builtin_enum_variant_name(&self, name: &str) -> bool {
        matches!(
            name,
            "Some"
                | "None"
                | "Ok"
                | "Err"
                | "Closed"
                | "Cancelled"
                | "TimedOut"
                | "Full"
                | "Item"
                | "Ready"
        )
    }

    fn type_check_builtin_enum_variant_constructor(
        &self,
        enum_name: &str,
        variant_name: &str,
        enum_ty: &Type,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let Some(variant_payload) =
            self.builtin_enum_variant_payload(enum_ty, enum_name, variant_name)
        else {
            return Err(Diagnostic::at(
                span,
                format!("enum `{}` has no variant `{}`", enum_name, variant_name),
            ));
        };
        if variant_payload.is_empty() {
            if !args.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` does not take a payload",
                        variant_name, enum_name
                    ),
                ));
            }
        } else {
            if args.len() != variant_payload.len() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                        variant_name,
                        enum_name,
                        variant_payload.len(),
                        if variant_payload.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                ));
            }
            for (payload_ty, argument) in variant_payload.iter().zip(args) {
                self.reject_owned_view_value(&argument.value, locals, "an enum payload")?;
                let actual = self.type_of_expr_hint(&argument.value, locals, Some(payload_ty))?;
                if actual != *payload_ty {
                    return Err(Diagnostic::at(
                        argument.span,
                        format!(
                            "variant `{}` of enum `{}` expects `{}`, found `{}`",
                            variant_name, enum_name, payload_ty, actual
                        ),
                    ));
                }
                if type_contains_loan_closure(&actual) {
                    return Err(Diagnostic::coded_at(
                        "AU3010",
                        argument.value.span,
                        "a closure containing a live view cannot be stored in an enum payload",
                    ));
                }
                self.consume_value_expr(&argument.value, locals)?;
            }
        }
        Ok(enum_ty.clone())
    }

    fn require_with_resource(&self, value_ty: &Type, span: crate::diag::Span) -> Result<()> {
        let Type::Named(name, args) = value_ty else {
            return Err(Diagnostic::at(
                span,
                format!("`with` requires a class resource, found `{}`", value_ty),
            ));
        };
        if is_builtin_io_resource_type(name, args) {
            return Ok(());
        }
        if !args.is_empty() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`with` does not yet support generic resource types in the bootstrap compiler, found `{}`",
                    value_ty
                ),
            ));
        }

        let Some(class_info) = self.classes.get(name) else {
            return Err(Diagnostic::at(
                span,
                format!("`with` requires a class resource, found `{}`", value_ty),
            ));
        };

        let Some(method) = class_info.methods.get("close") else {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "class `{}` cannot be used with `with` because it does not define `close(mut self)`",
                    name
                ),
            ));
        };

        if method.decl.receiver != Some(ReceiverKind::BorrowMut)
            || !method.signature.params.is_empty()
            || method.signature.return_type != Type::Unit
        {
            return Err(Diagnostic::at(
                method.decl.span,
                format!(
                    "`with` resources must define `close(mut self)` returning `None`; `{}` does not",
                    name
                ),
            ));
        }

        Ok(())
    }

    fn variant_payload_argument<'b>(
        &self,
        args: &'b [Argument],
        span: crate::diag::Span,
        variant_name: &str,
        enum_name: &str,
    ) -> Result<&'b Argument> {
        if args.len() != 1 {
            return Err(Diagnostic::at(
                span,
                format!(
                    "variant `{}` of enum `{}` expects exactly one payload argument",
                    variant_name, enum_name
                ),
            ));
        }
        if let Some(name) = args[0].name.as_deref() {
            if name != "value" {
                return Err(Diagnostic::at(
                    args[0].span,
                    format!(
                        "variant `{}` of enum `{}` only accepts the keyword `value=`",
                        variant_name, enum_name
                    ),
                ));
            }
        }
        Ok(&args[0])
    }

    fn variant_payload_arguments<'b>(
        &self,
        args: &'b [Argument],
        span: crate::diag::Span,
        variant_name: &str,
        enum_name: &str,
        payloads: &[EnumPayloadFieldInfo],
        named_payloads: bool,
    ) -> Result<Vec<&'b Argument>> {
        if payloads.is_empty() {
            if !args.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` does not take a payload",
                        variant_name, enum_name
                    ),
                ));
            }
            return Ok(Vec::new());
        }

        let uses_named_args = args.iter().any(|argument| argument.name.is_some());
        if uses_named_args {
            if payloads.len() == 1 && !named_payloads {
                let argument =
                    self.variant_payload_argument(args, span, variant_name, enum_name)?;
                return Ok(vec![argument]);
            }
            if !named_payloads {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` uses positional payloads and cannot be constructed with named arguments",
                        variant_name, enum_name
                    ),
                ));
            }
            let payload_names = payloads
                .iter()
                .map(|payload| {
                    payload.name.as_deref().ok_or_else(|| {
                        Diagnostic::at(
                            span,
                            format!(
                                "internal error: named enum payload metadata for `{}.{}` is missing its field name",
                                enum_name, variant_name
                            ),
                        )
                    })
                })
                .collect::<Result<BTreeSet<_>>>()?;
            if args.len() > payloads.len() {
                if let Some(extra) = args
                    .iter()
                    .filter_map(|argument| argument.name.as_deref())
                    .find(|name| !payload_names.contains(name))
                {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "variant `{}` of enum `{}` has no payload named `{}`",
                            variant_name, enum_name, extra
                        ),
                    ));
                }
            }
            if args.len() != payloads.len() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                        variant_name,
                        enum_name,
                        payloads.len(),
                        if payloads.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                ));
            }
            let mut ordered = Vec::with_capacity(payloads.len());
            for payload in payloads {
                let payload_name = payload.name.as_deref().expect(
                    "named enum payload metadata should have been validated before ordering",
                );
                let argument = args
                    .iter()
                    .find(|argument| argument.name.as_deref() == Some(payload_name))
                    .ok_or_else(|| {
                        Diagnostic::at(
                            span,
                            format!(
                                "variant `{}` of enum `{}` is missing payload argument `{}`",
                                variant_name, enum_name, payload_name
                            ),
                        )
                    })?;
                ordered.push(argument);
            }
            return Ok(ordered);
        }

        if args.len() != payloads.len() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                    variant_name,
                    enum_name,
                    payloads.len(),
                    if payloads.len() == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
        }
        Ok(args.iter().collect())
    }
}

#[cfg(test)]
#[path = "sema_tests.rs"]
mod tests;
