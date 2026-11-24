//! Computed property writes lower to ordinary mutable method calls.

use super::{BTreeMap, Diagnostic, MethodInfo, Result, Type};
use crate::ast::{
    Argument, AssignStmt, AssignTarget, Expr, ExprKind, ExprStmt, FunctionDecl, IfBranch, IfStmt,
    ParamMode, ReceiverKind, Stmt, ViewStmt, PROPERTY_SETTER_SUFFIX,
};
use crate::diag::{Span, GENERATED_COLUMN_STRIDE};

pub(super) fn validate_setter(
    name: &str,
    setter: &FunctionDecl,
    methods: &BTreeMap<String, MethodInfo>,
) -> Result<()> {
    let invalid = |rule: &str| {
        Diagnostic::coded_at("AU2064", setter.span, format!("setter for `{name}` {rule}"))
    };
    let getter = methods
        .get(name)
        .filter(|method| method.decl.property.is_some())
        .ok_or_else(|| invalid("requires a matching @property getter"))?;
    let signature = &methods[&setter.name].signature;
    if setter.receiver != Some(ReceiverKind::BorrowMut) {
        return Err(invalid("requires a `mut self` receiver"));
    }
    if !setter.type_params.is_empty()
        || setter.view_return.is_some()
        || signature.return_type != Type::Unit
    {
        return Err(invalid("cannot be generic or return a value"));
    }
    if signature.params.as_slice() != std::slice::from_ref(&getter.signature.return_type)
        || setter.params.iter().any(|param| {
            param.keyword_only || param.default.is_some() || param.mode == ParamMode::BorrowMut
        })
    {
        return Err(invalid("must take one value of the getter's result type"));
    }
    Ok(())
}

/// Lower a write after the checker has resolved the property on its receiver.
/// Each generated call needs a distinct position for the typed call tables.
pub(super) fn lower_write(assign: &AssignStmt, object: &Expr, field: &str) -> Stmt {
    let span = assign.member_span.unwrap_or(assign.span);
    let mut body = Vec::new();
    let mut receiver = object.clone();
    let mut value = assign.value.clone();
    if let Some(op) = assign.op {
        // Indexing can execute user code. Capture the selected place once
        // and end the exclusive view at the end of this assignment.
        if !fixed_place(object) {
            let name = "property receiver".to_owned();
            body.push(Stmt::View(ViewStmt {
                name: name.clone(),
                source: receiver,
                mutable: true,
                span,
            }));
            receiver = Expr {
                kind: ExprKind::Name(name),
                span,
            };
        }
        let getter_span = Span::new(span.line, span.column + GENERATED_COLUMN_STRIDE);
        let getter = Expr {
            kind: ExprKind::Member {
                object: Box::new(receiver.clone()),
                field: field.to_owned(),
            },
            span: getter_span,
        };
        let name = "property value".to_owned();
        body.push(Stmt::Assign(AssignStmt {
            mutable: false,
            member_span: None,
            annotation: None,
            op: None,
            span: getter_span,
            target: AssignTarget::Name(name.clone()),
            value: Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(getter),
                    right: Box::new(value),
                },
                span: Span::new(span.line, span.column + 2 * GENERATED_COLUMN_STRIDE),
            },
        }));
        value = Expr {
            kind: ExprKind::Name(name),
            span: getter_span,
        };
    }

    let setter = Stmt::Expr(ExprStmt {
        discarded: false,
        span,
        expr: Expr {
            kind: ExprKind::Call {
                callee: Box::new(Expr {
                    kind: ExprKind::Member {
                        object: Box::new(receiver),
                        field: format!("{field}{PROPERTY_SETTER_SUFFIX}"),
                    },
                    span,
                }),
                args: vec![Argument {
                    name: None,
                    span: value.span,
                    value,
                }],
            },
            span,
        },
    });
    if body.is_empty() {
        return setter;
    }
    body.push(setter);
    Stmt::If(IfStmt {
        branches: vec![IfBranch {
            condition: Expr {
                kind: ExprKind::Bool(true),
                span,
            },
            body,
            span,
        }],
        else_body: None,
        span,
    })
}

fn fixed_place(object: &Expr) -> bool {
    match &object.kind {
        ExprKind::Name(_) => true,
        ExprKind::Member { object, .. } => fixed_place(object),
        _ => false,
    }
}
