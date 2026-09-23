//! Arm-scoped `lookup` on lists and dictionaries (ADR-0061 C1).
//!
//! `match [mut] table.lookup(key):` never produces a value. Its `Found`
//! payload is an element or entry view that lives only inside the arm. The
//! checker and MIR lowering both rewrite the statement into forms they
//! already check and run:
//!
//! ```text
//! key = <key>                        # only when the key is not a simple place
//! if <key present in table>:
//!     view [mut] name = table[key]   # omitted for `Found(_)`
//!     <Found body>
//! else:
//!     <Missing or `_` body>
//! ```
//!
//! A bare `lookup` of a Copy element binds `name = table[key]` instead: a
//! copy reads the same as a shared view, and it can be returned or stored.
//! `match mut` always binds a mutable view, so writes reach the element.
//!
//! The key is evaluated once. A simple key place is read in place, so a
//! non-Copy key is never consumed.

use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, Expr, ExprKind, IfBranch, IfStmt, MatchStmt,
    Pattern, ReceiverKind, Stmt, UnaryOp, ViewStmt,
};
use crate::diag::{Diagnostic, Result, Span};

use super::Type;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LookupCollection {
    List,
    Dict,
}

impl LookupCollection {
    /// The collection kind and its element (or value) type.
    pub(crate) fn of_type(ty: &Type) -> Option<(Self, &Type)> {
        match ty {
            Type::Named(name, args) => match (name.as_str(), args.as_slice()) {
                ("list", [element]) => Some((Self::List, element)),
                ("dict", [_, value]) => Some((Self::Dict, value)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn is_type_name(name: &str) -> bool {
        matches!(name, "list" | "dict")
    }
}

/// The receiver and arguments of a `receiver.lookup(...)` call.
pub(crate) fn lookup_call(expr: &Expr) -> Option<(&Expr, &[Argument])> {
    let ExprKind::Call { callee, args } = &expr.kind else {
        return None;
    };
    let ExprKind::Member { object, field } = &callee.kind else {
        return None;
    };
    (field == "lookup").then_some((object.as_ref(), args.as_slice()))
}

/// The guidance shared by every refusal of `lookup` outside a `match`.
pub(crate) fn lookup_outside_match(span: Span) -> Diagnostic {
    Diagnostic::coded_at(
        "AU2005",
        span,
        "`lookup` is only available as the subject of a `match` statement",
    )
    .with_help(
        "write `match table.lookup(key):` with `case Lookup.Found(item):` and `case Lookup.Missing:` arms; use `get` for an owned clone, or `remove` to take the value out",
    )
}

enum ArmKind<'a> {
    Found(Option<&'a crate::ast::BindingPattern>),
    Missing,
    Rest,
}

fn arm_kind(pattern: &Pattern) -> Option<ArmKind<'_>> {
    match pattern {
        Pattern::Variant(variant)
            if variant
                .enum_name
                .as_deref()
                .is_none_or(|name| name == "Lookup") =>
        {
            match (
                variant.variant_name.as_str(),
                variant.subpatterns.as_slice(),
            ) {
                ("Found", [Pattern::Binding(binding)]) => Some(ArmKind::Found(Some(binding))),
                ("Found", [Pattern::Wildcard(_)]) => Some(ArmKind::Found(None)),
                ("Missing", []) => Some(ArmKind::Missing),
                _ => None,
            }
        }
        Pattern::Binding(binding) if binding.name == "Missing" => Some(ArmKind::Missing),
        Pattern::Wildcard(_) => Some(ArmKind::Rest),
        _ => None,
    }
}

fn is_simple_key(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Name(_) | ExprKind::Int(_) | ExprKind::Bool(_) => true,
        ExprKind::Member { object, .. } => is_simple_key(object),
        ExprKind::Group(inner) => is_simple_key(inner),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => matches!(expr.kind, ExprKind::Int(_)),
        _ => false,
    }
}

fn is_place(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Name(_) => true,
        ExprKind::Member { object, .. } => is_place(object),
        ExprKind::Group(inner) => is_place(inner),
        _ => false,
    }
}

fn expr(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

fn binary(op: BinaryOp, left: Expr, right: Expr, span: Span) -> Expr {
    expr(
        ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span,
    )
}

fn table_len(table: &Expr, span: Span) -> Expr {
    expr(
        ExprKind::Call {
            callee: Box::new(expr(
                ExprKind::Member {
                    object: Box::new(table.clone()),
                    field: "len".to_string(),
                },
                span,
            )),
            args: Vec::new(),
        },
        span,
    )
}

/// Rewrite a `lookup` match into a key binding and a presence test.
///
/// `collection` and `copy_element` come from the checked or inferred type of
/// the receiver. The caller decides them, because the checker and the
/// lowering each have their own view of expression types.
pub(crate) fn desugar_lookup_match(
    match_stmt: &MatchStmt,
    collection: LookupCollection,
    copy_element: bool,
) -> Result<Vec<Stmt>> {
    let (table, args) =
        lookup_call(&match_stmt.scrutinee).expect("desugar_lookup_match needs a lookup call");
    let span = match_stmt.scrutinee.span;
    if match_stmt.capability == ReceiverKind::Value {
        return Err(Diagnostic::coded_at(
            "AU2005",
            match_stmt.span,
            "`match own` cannot take a value out through `lookup`",
        )
        .with_help("use `remove(key)` to move the value out, or drop `own` to view it in place"));
    }
    let [key_arg] = args else {
        return Err(Diagnostic::coded_at(
            "AU2004",
            span,
            format!(
                "`lookup` takes exactly one {}, found {} arguments",
                match collection {
                    LookupCollection::List => "position",
                    LookupCollection::Dict => "key",
                },
                args.len()
            ),
        ));
    };
    if key_arg.name.is_some() {
        return Err(Diagnostic::coded_at(
            "AU2004",
            key_arg.span,
            "`lookup` does not take keyword arguments",
        ));
    }
    if !is_place(table) {
        return Err(Diagnostic::coded_at(
            "AU2005",
            table.span,
            "`lookup` needs a named list or dictionary, because its `Found` arm views the stored element in place",
        )
        .with_help("bind the collection to a name first, then `match name.lookup(key):`"));
    }

    let mut found = None;
    let mut missing = None;
    let mut rest = None;
    for arm in &match_stmt.arms {
        if let Some(guard) = &arm.guard {
            return Err(Diagnostic::coded_at(
                "AU2005",
                guard.span,
                "`lookup` arms do not take guards",
            )
            .with_help("test the condition inside the arm body instead"));
        }
        let Some(kind) = arm_kind(&arm.pattern) else {
            return Err(Diagnostic::coded_at(
                "AU2002",
                arm.span,
                "a `lookup` arm must be `Lookup.Found(name)`, `Lookup.Found(_)`, `Lookup.Missing`, or `_`",
            ));
        };
        // Arms after `_`, repeated arms, and a `_` after both `Found` and
        // `Missing` can never run.
        let unreachable = rest.is_some()
            || match kind {
                ArmKind::Found(binding) => {
                    let repeated = found.is_some();
                    found.get_or_insert((binding, arm));
                    repeated
                }
                ArmKind::Missing => missing.replace(arm).is_some(),
                ArmKind::Rest => {
                    rest = Some(arm);
                    found.is_some() && missing.is_some()
                }
            };
        if unreachable {
            return Err(Diagnostic::coded_at(
                "AU2002",
                arm.span,
                "this `lookup` arm can never run; an earlier arm already covers it",
            ));
        }
    }
    let Some((binding, found_arm)) = found else {
        return Err(Diagnostic::coded_at(
            "AU2002",
            match_stmt.span,
            "a `lookup` match needs a `Lookup.Found` arm",
        )
        .with_help("use `if key in table:` when only presence matters"));
    };
    let Some(absent_arm) = missing.or(rest) else {
        return Err(Diagnostic::coded_at(
            "AU2002",
            match_stmt.span,
            "a `lookup` match must also handle `Lookup.Missing`",
        )
        .with_help("add `case Lookup.Missing:` or `case _:`"));
    };

    let mut stmts = Vec::new();
    let key = if is_simple_key(&key_arg.value) {
        key_arg.value.clone()
    } else {
        let hidden = format!(
            "__lookup_key_{}_{}",
            key_arg.value.span.line, key_arg.value.span.column
        );
        stmts.push(Stmt::Assign(AssignStmt {
            mutable: false,
            target: AssignTarget::Name(hidden.clone()),
            annotation: None,
            op: None,
            value: key_arg.value.clone(),
            span: key_arg.span,
        }));
        expr(ExprKind::Name(hidden), key_arg.value.span)
    };

    let presence = match collection {
        LookupCollection::Dict => expr(
            ExprKind::Membership {
                value: Box::new(key.clone()),
                container: Box::new(table.clone()),
                negated: false,
                operator_span: span,
            },
            span,
        ),
        LookupCollection::List => binary(
            BinaryOp::And,
            binary(BinaryOp::Less, key.clone(), table_len(table, span), span),
            binary(
                BinaryOp::GreaterEq,
                key.clone(),
                expr(
                    ExprKind::Unary {
                        op: UnaryOp::Neg,
                        expr: Box::new(table_len(table, span)),
                    },
                    span,
                ),
                span,
            ),
            span,
        ),
    };

    let mut found_body = Vec::with_capacity(found_arm.body.len() + 1);
    if let Some(binding) = binding {
        let element = expr(
            ExprKind::Index {
                object: Box::new(table.clone()),
                index: Box::new(key),
            },
            binding.span,
        );
        let mutable = match_stmt.capability == ReceiverKind::BorrowMut;
        found_body.push(if copy_element && !mutable {
            Stmt::Assign(AssignStmt {
                mutable: false,
                target: AssignTarget::Name(binding.name.clone()),
                annotation: None,
                op: None,
                value: element,
                span: binding.span,
            })
        } else {
            Stmt::View(ViewStmt {
                name: binding.name.clone(),
                mutable,
                source: element,
                span: binding.span,
            })
        });
    }
    found_body.extend(found_arm.body.iter().cloned());

    stmts.push(Stmt::If(IfStmt {
        branches: vec![IfBranch {
            condition: presence,
            body: found_body,
            span: found_arm.span,
        }],
        else_body: Some(absent_arm.body.clone()),
        span: match_stmt.span,
    }));
    Ok(stmts)
}
