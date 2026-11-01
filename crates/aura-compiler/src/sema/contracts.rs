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
use crate::ast::{ComprehensionOutput, ContractClause, ContractKind, FormatPart};
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
            }
            self.check_clause_condition(clause, &mut scope)?;
        }
        Ok(())
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
        reject_impure_condition(&clause.condition, clause.kind, &mut receivers)?;
        let (expected, what) = match clause.kind {
            ContractKind::Decreases => (Type::named("int64"), "measure"),
            _ => (Type::named("bool"), "condition"),
        };
        let actual = self.type_of_expr_hint(&clause.condition, scope, Some(&expected))?;
        if actual != expected {
            return Err(Diagnostic::coded_at(
                "AU2002",
                clause.condition.span,
                format!(
                    "{} `{}` {what} must be `{expected}`, found `{actual}`",
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
            let ty = self.type_of_expr(receiver, scope)?;
            if !has_builtin_methods(&ty) {
                return Err(impure(
                    receiver,
                    clause.kind,
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

fn impure(expr: &Expr, kind: ContractKind, what: &str) -> Diagnostic {
    Diagnostic::coded_at(
        CODE,
        expr.span,
        format!(
            "{} `{}` condition must not change program state; {what} is not allowed here",
            if kind == ContractKind::Invariant { "an" } else { "a" },
            kind.keyword()
        ),
    )
    .with_help("conditions may read values and call pure builtins such as `len`, `min`, `all`, and read-only methods such as `contains`")
}

/// Rejects a condition that could change program state.
///
/// Method calls are allowed by name here; `receivers` collects each call's
/// receiver so the caller can check that it has a builtin type.
fn reject_impure_condition<'e>(
    expr: &'e Expr,
    kind: ContractKind,
    receivers: &mut Vec<&'e Expr>,
) -> Result<()> {
    macro_rules! check {
        ($expr:expr) => {
            reject_impure_condition($expr, kind, receivers)
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
                ExprKind::Name(name) => PURE_FUNCTIONS.contains(&name.as_str()),
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
