//! Lower writes to a union member through the existing byte reinterpretation
//! boundary. The member can be shorter than the union, so copy its prefix.
use super::*;

pub(super) fn complete_lengths(classes: &mut BTreeMap<String, ClassInfo>) {
    let lengths = classes
        .iter()
        .filter(|(_, class)| class.decl.c_union.is_some())
        .filter_map(|(name, _)| {
            Some((
                name.clone(),
                ffi_struct_layout(&Type::named(name), classes)?,
            ))
        })
        .collect::<Vec<_>>();
    for (name, layout) in lengths {
        let class = classes.get_mut(&name).expect("union class just resolved");
        for field in layout.fields {
            let setter = format!("__write_{}", field.name);
            if let Some(method) = class.methods.get_mut(&setter) {
                if let [_, Stmt::For(loop_stmt)] = method.decl.body.as_mut_slice() {
                    if let ExprKind::Call { args, .. } = &mut loop_stmt.iterable.kind {
                        args[0].value.kind = ExprKind::Int(field.kind.size() as u128);
                    }
                }
            }
        }
    }
}

pub(super) fn setter_statement(assign: &crate::ast::AssignStmt) -> Option<Stmt> {
    let crate::ast::AssignTarget::Member { object, field } = &assign.target else {
        return None;
    };
    let span = assign.span;
    let member = Expr {
        span,
        kind: ExprKind::Member {
            object: object.clone(),
            field: format!("__write_{field}"),
        },
    };
    let value = assign.value.clone();
    let call = Expr {
        span,
        kind: ExprKind::Call {
            callee: Box::new(member),
            args: vec![Argument {
                span: value.span,
                name: None,
                value,
            }],
        },
    };
    Some(Stmt::Expr(crate::ast::ExprStmt {
        expr: call,
        span,
        discarded: false,
    }))
}
