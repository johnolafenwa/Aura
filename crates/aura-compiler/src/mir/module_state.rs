//! A module cell is an explicit memory operation in MIR. Every read produces
//! a snapshot, including reads separated by calls that can reenter the module.
use super::*;

impl Lowerer<'_> {
    pub(super) fn state_binding(&self, expr: &Expr) -> Option<&crate::sema::ConstantInfo> {
        let constant = match &grouped_mir_expr(expr).kind {
            ExprKind::Name(name) => self.resolve_constant_info(name)?,
            ExprKind::Member { object, field } => self
                .module_namespace(&self.infer_module_path(object)?)?
                .constants
                .get(field)?,
            _ => return None,
        };
        constant.decl.mutable.then_some(constant)
    }

    fn state_root<'e>(&self, expr: &'e Expr) -> Option<&'e Expr> {
        if self.state_binding(expr).is_some() {
            return Some(expr);
        }
        match &grouped_mir_expr(expr).kind {
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                self.state_root(object)
            }
            _ => None,
        }
    }

    /// Captures each subscript before evaluating the right hand side. The root
    /// for a projected store is read *after* that evaluation so side effects in
    /// the right hand side cannot roll back writes to sibling fields.
    fn capture_state_target(&mut self, expr: &mut Expr) {
        if self.state_binding(expr).is_some() {
            return;
        }
        match &mut expr.kind {
            ExprKind::Member { object, .. } | ExprKind::Group(object) => {
                self.capture_state_target(object)
            }
            ExprKind::Index { object, index } => {
                self.capture_state_target(object);
                let local = self.new_temp_for_expr(index);
                let value = self.lower_expr(index);
                self.emit(Instruction::Assign {
                    target: local.clone(),
                    value: Rvalue::Use(value),
                });
                index.kind = ExprKind::Name(local);
            }
            _ => {}
        }
    }

    fn state_union_member_type(&self, target: &AssignTarget) -> Option<Type> {
        let AssignTarget::Member { object, field } = target else {
            return None;
        };
        let Type::Named(name, _) = self.infer_expr_type(object)? else {
            return None;
        };
        let class = self.resolve_class_info(&name)?;
        class.decl.c_union.as_ref()?;
        let method = format!("__write_{field}");
        class.methods.get(&method)?.signature.params.last().cloned()
    }

    pub(super) fn lower_state_assignment(&mut self, assign: &AssignStmt) -> bool {
        let kind = match &assign.target {
            AssignTarget::Name(name) => ExprKind::Name(name.clone()),
            AssignTarget::Member { object, field } => ExprKind::Member {
                object: object.clone(),
                field: field.clone(),
            },
            AssignTarget::Index { object, index } => ExprKind::Index {
                object: object.clone(),
                index: index.clone(),
            },
        };
        let mut target = Expr {
            kind,
            span: assign.span,
        };
        let Some(root) = self.state_root(&target) else {
            return false;
        };
        let constant = self.state_binding(root).unwrap().clone();
        let root_span = root.span;
        let whole = self.state_binding(&target).is_some();
        let member_ty = self.state_union_member_type(&assign.target);
        let expected = member_ty
            .clone()
            .or_else(|| self.infer_expr_type(&target))
            .unwrap_or_else(|| constant.ty.clone());
        self.capture_state_target(&mut target);
        let rhs = match assign.op {
            Some(op) => Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(target.clone()),
                    right: Box::new(assign.value.clone()),
                },
                span: assign.span,
            },
            None => assign.value.clone(),
        };
        let value = self.lower_expr_for_owned_value(&rhs, Some(&expected));
        let key = cell_key(&constant);
        let value = if whole {
            value
        } else {
            let stored = self.new_typed_temp(expected);
            self.emit(Instruction::Assign {
                target: stored.clone(),
                value: Rvalue::Use(value),
            });
            let snapshot = self.new_typed_temp(constant.ty.clone());
            self.emit(Instruction::Assign {
                target: snapshot.clone(),
                value: Rvalue::ModuleLoad { key: key.clone() },
            });
            let mut cursor = &mut target;
            while self.state_binding(cursor).is_none() {
                cursor = match &mut cursor.kind {
                    ExprKind::Index { object, .. }
                    | ExprKind::Member { object, .. }
                    | ExprKind::Group(object) => object,
                    _ => unreachable!("validated module storage path"),
                };
            }
            *cursor = Expr {
                kind: ExprKind::Name(snapshot.clone()),
                span: root_span,
            };
            let target = match target.kind {
                ExprKind::Index { object, index } => AssignTarget::Index { object, index },
                ExprKind::Member { object, field } => AssignTarget::Member { object, field },
                _ => unreachable!("projected module store"),
            };
            let write = AssignStmt {
                target,
                value: Expr {
                    kind: ExprKind::Name(stored),
                    span: rhs.span,
                },
                mutable: false,
                op: None,
                annotation: None,
                span: assign.span,
            };
            if member_ty.is_some() {
                let Some(Stmt::Expr(call)) = crate::sema::state_union_setter_statement(&write)
                else {
                    unreachable!()
                };
                self.lower_expr(&call.expr);
            } else {
                self.lower_assign(&write);
            }
            Operand::Place(snapshot)
        };
        let unit = self.new_typed_temp(Type::Unit);
        self.emit(Instruction::Assign {
            target: unit,
            value: Rvalue::ModuleStore { key, value },
        });
        true
    }
}

pub(super) fn cell_key(binding: &crate::sema::ConstantInfo) -> String {
    format!("{}::{}", binding.module_name, binding.decl.name)
}
