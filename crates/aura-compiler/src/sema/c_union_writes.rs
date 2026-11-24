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
        if class.decl.layout_controls.inline_union {
            let size = layout.size();
            let field = &mut class.decl.fields[0];
            let span = field.span;
            let element = TypeRef::named("uint8", vec![], false, span);
            let count = TypeRef::named(size.to_string(), vec![], false, span);
            field.ty = TypeRef::named("fixed", vec![element, count], false, span);
            let fixed = Type::Named(
                "fixed".into(),
                vec![Type::named("uint8"), Type::named(size.to_string())],
            );
            class
                .fields
                .get_mut(&field.name)
                .expect("union bytes field")
                .ty = fixed;
            field.default = Some(Expr {
                span,
                kind: ExprKind::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(Expr {
                        span,
                        kind: ExprKind::List(vec![Expr {
                            span,
                            kind: ExprKind::Int(0),
                        }]),
                    }),
                    right: Box::new(Expr {
                        span,
                        kind: ExprKind::Int(size as u128),
                    }),
                },
            });
        }
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

pub(crate) fn setter_statement(assign: &crate::ast::AssignStmt) -> Option<Stmt> {
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
