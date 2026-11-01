//! Contract clauses (verification checkpoint, stage V1): `requires` and
//! `ensures` on functions, `invariant` and `decreases` on loops.
//!
//! A clause's condition is a specification expression: it must have type
//! `bool` and must not change program state. It may read parameters, `self`,
//! module constants, fields, and elements, and call the pure builtins and
//! read-only builtin methods listed below. Inside `ensures`, `result` names
//! the value the function returns.
//!
//! The checker types each condition against a copy of the entry scope, so a
//! clause never changes the ownership state the body starts from.

use super::{
    BTreeMap, Diagnostic, Expr, ExprKind, FunctionChecker, FunctionDecl, HashMap, LocalBinding,
    Result, Type,
};
use crate::ast::{ComprehensionOutput, ContractClause, ContractKind, FormatPart, OldValue};
use aura_core::ops::ReceiverKind;

const CODE: &str = "AU2048";

/// The name the return value takes inside an `ensures` condition.
pub(crate) const RESULT_NAME: &str = "result";

/// Builtin functions a condition may call. Each reads its arguments only.
const PURE_FUNCTIONS: &[&str] = &[
    "len", "abs", "min", "max", "sum", "any", "all", "ord", "chr", "str", "round", "sqrt",
    "sorted", "reversed", "list", "divmod", "range",
];

/// Builtin methods a condition may call. Each reads its receiver and
/// arguments only.
const PURE_METHODS: &[&str] = &[
    "len",
    "byte_len",
    "contains",
    "starts_with",
    "ends_with",
    "find",
    "count",
    "is_empty",
    "keys",
    "values",
    "items",
    "get",
    "index",
    "to_lower",
    "to_upper",
    "trim",
    "trim_start",
    "trim_end",
    "split",
    "lines",
    "chars",
    "split_whitespace",
    "to_string",
    "to_float",
    "copy",
    "clone",
    "shape",
    "mean",
];

impl FunctionChecker<'_> {
    /// Checks every clause of `function` against its entry scope.
    pub(super) fn check_contracts(
        &self,
        function: &FunctionDecl,
        locals: &HashMap<String, LocalBinding>,
        return_type: &Type,
    ) -> Result<()> {
        for clause in &function.contracts {
            if clause.kind == ContractKind::Ensures {
                if function.view_return.is_some() {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "a function with a view result cannot declare `ensures` yet",
                    ));
                }
                if function.name == super::INITIALIZER_NAME {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "`__init__` cannot declare `ensures` yet; state the promise as a class invariant later, or check it in a factory",
                    ));
                }
                if function
                    .params
                    .iter()
                    .any(|param| param.name == RESULT_NAME)
                {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "`result` names the return value in an `ensures` clause; rename the parameter `result`",
                    ));
                }
                if let Some(param) = function.params.iter().find(|param| {
                    locals.get(&param.name).is_some_and(|binding| {
                        binding.passing == ReceiverKind::Value && !self.is_copy_type(&binding.ty)
                    }) && super::expr_references_name(&clause.condition, &param.name)
                }) {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        format!(
                            "an `ensures` clause cannot read `own` parameter `{}`, which the body may move",
                            param.name
                        ),
                    ));
                }
            }
            let mut scope = locals.clone();
            if clause.kind == ContractKind::Ensures {
                scope.insert(RESULT_NAME.to_string(), result_binding(return_type.clone()));
                for old in &clause.olds {
                    let ty = self.check_old_value(function, old, locals)?;
                    scope.insert(old.name.clone(), result_binding(ty));
                }
            }
            self.check_clause_condition(clause, &mut scope)?;
        }
        Ok(())
    }

    /// Checks one `old(expr)`: a pure read of a parameter or `self`, or of
    /// a field or element of one, whose value can be copied on entry.
    fn check_old_value(
        &self,
        function: &FunctionDecl,
        old: &OldValue,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let reads_parameter = old_root(&old.expr).is_some_and(|root| {
            root == "self" || function.params.iter().any(|param| param.name == root)
        });
        if !reads_parameter {
            return Err(Diagnostic::coded_at(
                CODE,
                old.expr.span,
                "`old` reads a parameter or `self`, or a field or element of one",
            ));
        }
        self.reject_impure(&old.expr, "an `old` value", &mut Vec::new())?;
        let ty = self.with_spec(|| self.type_of_expr(&old.expr, &mut locals.clone()))?;
        let copyable = self.is_copy_type(&ty)
            || matches!(&ty, Type::Named(name, _) if matches!(name.as_str(), "str" | "list" | "dict" | "set" | "Array"));
        if !copyable {
            return Err(Diagnostic::coded_at(
                CODE,
                old.expr.span,
                format!(
                    "`old` keeps a copy of its value, so it takes a Copy type, `str`, or a list, dict, set, or Array, not `{ty}`"
                ),
            ));
        }
        if !self.is_copy_type(&ty) {
            self.reject_rng_duplication("`old`", &ty, old.expr.span)?;
        }
        Ok(ty)
    }

    /// Checks a `spec def`: a module function whose body is one `return`
    /// of a pure expression, with no `mut` parameters and no clauses.
    pub(super) fn check_spec_function(
        &self,
        function: &FunctionDecl,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
    ) -> Result<()> {
        if let Some(param) = function.params.iter().find(|param| {
            locals
                .get(&param.name)
                .is_some_and(|binding| binding.passing == ReceiverKind::BorrowMut)
        }) {
            return Err(Diagnostic::coded_at(
                CODE,
                param.span,
                format!("a `spec def` cannot take `mut` parameter `{}`", param.name),
            ));
        }
        if let Some(clause) = function.contracts.first() {
            return Err(Diagnostic::coded_at(
                CODE,
                clause.span,
                "a `spec def` cannot declare contract clauses yet",
            ));
        }
        let [crate::ast::Stmt::Return(crate::ast::ReturnStmt {
            value: Some(value),
            view: None,
            ..
        })] = function.body.as_slice()
        else {
            return Err(Diagnostic::coded_at(
                CODE,
                function.span,
                "a `spec def` body is one `return` of a pure expression, after an optional docstring",
            ));
        };
        self.reject_impure(value, "a `spec def` body", &mut Vec::new())?;
        self.with_spec(|| {
            self.check_block(&function.body, locals, return_type, 0, true)
                .map(|_| ())
        })
    }

    /// Runs `check` with `spec def` references allowed.
    fn with_spec<T>(&self, check: impl FnOnce() -> Result<T>) -> Result<T> {
        let outer = self.in_spec.replace(true);
        let result = check();
        self.in_spec.set(outer);
        result
    }

    /// Rejects naming a `spec def` outside a contract clause or another
    /// `spec def` body.
    pub(super) fn reject_spec_reference(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if self.in_spec.get() {
            return Ok(());
        }
        let name = match &expr.kind {
            ExprKind::Name(name) => name,
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Name(name) => name,
                _ => return Ok(()),
            },
            _ => return Ok(()),
        };
        if locals.contains_key(name) || !self.is_spec_function(name) {
            return Ok(());
        }
        Err(Diagnostic::coded_at(
            CODE,
            expr.span,
            format!(
                "`{name}` is a `spec def`; only contract clauses and other `spec def` bodies may use it"
            ),
        ))
    }

    fn is_spec_function(&self, name: &str) -> bool {
        self.resolve_function_info(name)
            .is_some_and(|function| function.decl.spec)
    }

    /// [`reject_impure_condition`] with this module's `spec def` functions
    /// allowed.
    fn reject_impure<'e>(
        &self,
        expr: &'e Expr,
        what: &str,
        receivers: &mut Vec<&'e Expr>,
    ) -> Result<()> {
        reject_impure_condition(expr, what, receivers, &|name| self.is_spec_function(name))
    }

    /// Checks the clauses of `kinds` at the top of a loop body against the
    /// scope they are evaluated in.
    pub(super) fn check_loop_clauses(
        &self,
        clauses: &[ContractClause],
        kinds: &[ContractKind],
        locals: &HashMap<String, LocalBinding>,
        for_loop: bool,
    ) -> Result<()> {
        let mut measures = 0;
        for clause in clauses.iter().filter(|clause| kinds.contains(&clause.kind)) {
            if clause.kind == ContractKind::Decreases {
                if for_loop {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "a `for` loop always ends, so it takes no `decreases` clause; `decreases` belongs on `while`",
                    ));
                }
                measures += 1;
                if measures > 1 {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "a loop takes at most one `decreases` clause",
                    ));
                }
            }
            self.check_clause_condition(clause, &mut locals.clone())?;
        }
        Ok(())
    }

    /// Checks one clause's condition: pure, and `bool` (or `int64` for a
    /// `decreases` measure).
    fn check_clause_condition(
        &self,
        clause: &ContractClause,
        scope: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let mut receivers = Vec::new();
        let what = format!(
            "{} `{}` condition",
            if clause.kind == ContractKind::Invariant {
                "an"
            } else {
                "a"
            },
            clause.kind.keyword()
        );
        self.reject_impure(&clause.condition, &what, &mut receivers)?;
        let (expected, noun) = match clause.kind {
            ContractKind::Decreases => (Type::named("int64"), "measure"),
            _ => (Type::named("bool"), "condition"),
        };
        let actual =
            self.with_spec(|| self.type_of_expr_hint(&clause.condition, scope, Some(&expected)))?;
        if actual != expected {
            return Err(Diagnostic::coded_at(
                "AU2002",
                clause.condition.span,
                format!(
                    "{} `{}` {noun} must be `{expected}`, found `{actual}`",
                    if clause.kind == ContractKind::Invariant {
                        "an"
                    } else {
                        "a"
                    },
                    clause.kind.keyword()
                ),
            ));
        }
        // A method allowed by name must also be a builtin's: a user type's
        // method of the same name may change state.
        for receiver in receivers {
            let ty = self.with_spec(|| self.type_of_expr(receiver, scope))?;
            if !has_builtin_methods(&ty) {
                return Err(impure(
                    receiver,
                    &what,
                    &format!("a method call on `{ty}`, which may change program state,"),
                ));
            }
        }
        Ok(())
    }
}

/// True when `ty`'s methods are the builtin read-only ones of
/// [`PURE_METHODS`].
fn has_builtin_methods(ty: &Type) -> bool {
    match ty {
        Type::Named(name, _) => matches!(
            name.as_str(),
            "list" | "dict" | "set" | "str" | "bytes" | "Array" | "Span" | "Range"
        ),
        Type::Tuple(_) => true,
        _ => false,
    }
}

/// The read-only binding `result` has inside an `ensures` condition.
fn result_binding(ty: Type) -> LocalBinding {
    LocalBinding {
        ty,
        assignable: false,
        mutable_place: false,
        managed_resource: false,
        passing: ReceiverKind::Borrow,
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
        narrowed: BTreeMap::new(),
        stale_narrowing: BTreeMap::new(),
    }
}

fn impure(expr: &Expr, context: &str, what: &str) -> Diagnostic {
    Diagnostic::coded_at(
        CODE,
        expr.span,
        format!("{context} must not change program state; {what} is not allowed here"),
    )
    .with_help("conditions may read values and call pure builtins such as `len`, `min`, `all`, read-only methods such as `contains`, and `spec def` functions")
}

/// The parameter or `self` an `old` path starts from.
fn old_root(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Name(name) => Some(name),
        ExprKind::Member { object, .. }
        | ExprKind::Index { object, .. }
        | ExprKind::Slice { object, .. } => old_root(object),
        ExprKind::Group(inner) => old_root(inner),
        _ => None,
    }
}

/// Rejects a condition that could change program state.
///
/// Method calls are allowed by name here; `receivers` collects each call's
/// receiver so the caller can check that it has a builtin type.
fn reject_impure_condition<'e>(
    expr: &'e Expr,
    kind: &str,
    receivers: &mut Vec<&'e Expr>,
    is_spec: &dyn Fn(&str) -> bool,
) -> Result<()> {
    macro_rules! check {
        ($expr:expr) => {
            reject_impure_condition($expr, kind, receivers, is_spec)
        };
    }
    match &expr.kind {
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_) => Ok(()),
        ExprKind::FString(parts) => {
            for part in parts {
                match part {
                    FormatPart::Literal(_) => {}
                    FormatPart::Expr(expr) | FormatPart::Formatted { expr, .. } => check!(expr)?,
                }
            }
            Ok(())
        }
        ExprKind::Tuple(items) | ExprKind::List(items) | ExprKind::Set(items) => {
            for item in items {
                check!(item)?;
            }
            Ok(())
        }
        ExprKind::Map(entries) => {
            for entry in entries {
                check!(&entry.key)?;
                check!(&entry.value)?;
            }
            Ok(())
        }
        ExprKind::Comprehension { output, clauses } => {
            match output {
                ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                    check!(value)?
                }
                ComprehensionOutput::Map { key, value } => {
                    check!(key)?;
                    check!(value)?;
                }
            }
            for clause in clauses {
                check!(&clause.iterable)?;
                for filter in &clause.filters {
                    check!(filter)?;
                }
            }
            Ok(())
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } | ExprKind::Group(expr) => {
            check!(expr)
        }
        ExprKind::Binary { left, right, .. } => {
            check!(left)?;
            check!(right)
        }
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => {
            check!(then_expr)?;
            check!(condition)?;
            check!(else_expr)
        }
        ExprKind::Member { object, .. } => check!(object),
        ExprKind::Index { object, index } => {
            check!(object)?;
            check!(index)
        }
        ExprKind::Slice {
            object, start, end, ..
        } => {
            check!(object)?;
            for bound in start.iter().chain(end.iter()) {
                check!(bound)?;
            }
            Ok(())
        }
        ExprKind::Membership {
            value, container, ..
        } => {
            check!(value)?;
            check!(container)
        }
        ExprKind::CompareChain { first, links } => {
            check!(first)?;
            for link in links {
                check!(&link.operand)?;
            }
            Ok(())
        }
        ExprKind::IsNone { value, .. } => check!(value),
        ExprKind::Call { callee, args } => {
            let allowed = match &callee.kind {
                ExprKind::Name(name) if name == "old" => {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        expr.span,
                        "`old(...)` names a value on entry; it is allowed only in an `ensures` condition, around a parameter or a field or element of one",
                    ));
                }
                ExprKind::Name(name) if name == "forall" || name == "exists" => {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        expr.span,
                        format!("write `{name}(name in collection, condition)`"),
                    ));
                }
                ExprKind::Name(name) => PURE_FUNCTIONS.contains(&name.as_str()) || is_spec(name),
                ExprKind::Member { object, field } => {
                    check!(object)?;
                    receivers.push(object);
                    PURE_METHODS.contains(&field.as_str())
                }
                _ => false,
            };
            if !allowed {
                return Err(impure(
                    expr,
                    kind,
                    "a call to a user function or a method that may change its receiver",
                ));
            }
            for arg in args {
                check!(&arg.value)?;
            }
            Ok(())
        }
        ExprKind::Lambda { .. } => Err(impure(expr, kind, "a lambda")),
        ExprKind::Specialize { .. } => Err(impure(expr, kind, "an explicit specialization")),
        ExprKind::Try(_) => Err(impure(expr, kind, "`try`, which can return early,")),
        ExprKind::Match { .. } => Err(impure(expr, kind, "a `match` expression")),
    }
}
