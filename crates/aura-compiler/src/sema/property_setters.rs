//! Computed property writes lower to ordinary mutable method calls.

use super::property_reads::PropertySite;
use super::{
    BTreeMap, BTreeSet, Diagnostic, FunctionChecker, LocalBinding, MethodInfo, Result, Type,
};
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

#[derive(Default)]
pub(super) struct PropertyWrite {
    pub(super) stage_rhs: bool,
    borrowed_selectors: BTreeSet<PropertySite>,
}

impl FunctionChecker<'_> {
    /// A copied key must keep its value even if the RHS changes the binding.
    /// A non-copy key already in a place stays borrowed for both lookups.
    pub(super) fn property_write(
        &self,
        receiver: &Expr,
        value: &Expr,
        stage_rhs: bool,
        locals: &mut std::collections::HashMap<String, LocalBinding>,
    ) -> Result<PropertyWrite> {
        let mut borrowed_selectors = BTreeSet::new();
        let mut current = receiver;
        let mut indexed = false;
        loop {
            match &current.kind {
                ExprKind::Member { object, .. } => current = object,
                ExprKind::Index { object, index } => {
                    indexed = true;
                    if self.borrow_call_place(index).is_some()
                        && !self.is_copy_type(&self.type_of_expr(index, &mut locals.clone())?)
                        && self.property_read_name(index, locals).is_none()
                    {
                        borrowed_selectors.insert((index.span.line, index.span.column));
                    }
                    current = object;
                }
                _ => break,
            }
        }
        if indexed {
            if let Some(path) = self.borrow_call_place(receiver) {
                // The getter and setter must refer to the same element even
                // if a call on the RHS could rearrange the collection.
                let selection = super::BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Borrow,
                    param_name: "property assignment receiver".to_string(),
                    origin_span: receiver.span,
                };
                let mut accesses = Vec::new();
                self.collect_expr_borrowed_places(value, locals, &mut accesses)?;
                self.collect_expr_consumed_places(value, locals, &mut accesses)?;
                self.reject_retained_access_overlap(&[selection], &accesses)?;
            }
        }
        Ok(PropertyWrite {
            stage_rhs,
            borrowed_selectors,
        })
    }
}

/// Lower a write after the checker has resolved the property on its receiver.
/// Each generated call needs a distinct position for the typed call tables.
pub(super) fn lower_write(
    assign: &AssignStmt,
    object: &Expr,
    field: &str,
    write: &PropertyWrite,
) -> Stmt {
    let span = assign.member_span.unwrap_or(assign.span);
    let mut body = Vec::new();
    let mut receiver = object.clone();
    let mut value = assign.value.clone();
    if assign.op.is_some() || write.stage_rhs {
        stabilize_indices(&mut receiver, &mut body, &write.borrowed_selectors);
    }
    if let Some(op) = assign.op {
        let getter_span = Span::new(span.line, span.column + GENERATED_COLUMN_STRIDE);
        let getter = Expr {
            kind: ExprKind::Member {
                object: Box::new(receiver.clone()),
                field: field.to_owned(),
            },
            span: getter_span,
        };
        value = Expr {
            kind: ExprKind::Binary {
                op,
                left: Box::new(getter),
                right: Box::new(value),
            },
            span: Span::new(span.line, span.column + 2 * GENERATED_COLUMN_STRIDE),
        };
    }
    if assign.op.is_some() || write.stage_rhs {
        value = bind_temporary(&mut body, "property value".to_owned(), value);
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

/// Capture selectors before evaluating the RHS. An identifier or indexed
/// key may be a non-copy string, so borrow a place used as a selector.
fn stabilize_indices(receiver: &mut Expr, body: &mut Vec<Stmt>, borrowed: &BTreeSet<PropertySite>) {
    match &mut receiver.kind {
        ExprKind::Member { object, .. } => stabilize_indices(object, body, borrowed),
        ExprKind::Index { object, index } => {
            stabilize_indices(object, body, borrowed);
            if matches!(index.kind, ExprKind::Int(_) | ExprKind::String(_)) {
                return;
            }
            let name = format!("property index {}", body.len());
            let span = index.span;
            let source = index.as_ref().clone();
            **index = if borrowed.contains(&(span.line, span.column)) {
                body.push(Stmt::View(ViewStmt {
                    name: name.clone(),
                    mutable: false,
                    source,
                    span,
                }));
                Expr {
                    kind: ExprKind::Name(name),
                    span,
                }
            } else {
                bind_temporary(body, name, source)
            };
        }
        _ => {}
    }
}

fn bind_temporary(body: &mut Vec<Stmt>, name: String, value: Expr) -> Expr {
    let span = value.span;
    body.push(Stmt::Assign(AssignStmt {
        mutable: false,
        member_span: None,
        annotation: None,
        op: None,
        span,
        target: AssignTarget::Name(name.clone()),
        value,
    }));
    Expr {
        kind: ExprKind::Name(name),
        span,
    }
}
