//! Inline array operations use a typed instruction call. Each retains its
//! array type so an interpreter and a cross compiler agree on the length.
use super::*;
use crate::sema::fixed_arrays::{initializer, Initializer};

pub(crate) const LITERAL: &str = "#fixed.literal";
pub(crate) const REPEAT: &str = "#fixed.repeat";
pub(crate) const READ: &str = "#fixed.read";
pub(crate) const WRITE: &str = "#fixed.write";

pub(crate) fn is_operation(name: &str) -> bool {
    matches!(name, LITERAL | REPEAT | READ | WRITE)
}

fn call(op: &str, ty: &Type, values: Vec<Operand>) -> Rvalue {
    Rvalue::Call {
        callee: CallTarget::Name(op.to_owned()),
        args: values.into_iter().map(|value| MirArg { name: None, value, writeback_place: None }).collect(),
        type_args: BTreeMap::from([("array".to_string(), ty.clone())]),
    }
}

fn access(op: &str, ty: &Type, object: Operand, index: Operand, value: Option<Operand>, span: crate::diag::Span) -> Rvalue {
    let mut args = vec![object, index, Operand::Int(span.line as u128), Operand::Int(span.column as u128)];
    args.extend(value);
    call(op, ty, args)
}

/// An immutable module array can be materialized from its literal bytes.
pub(super) fn constant_initializer(expr: &Expr) -> bool {
    match initializer(expr) {
        Some(Initializer::Elements(values)) => values.iter().all(constant_initializer),
        Some(Initializer::Repeat(value, _)) => constant_initializer(value),
        None => match &expr.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
            ExprKind::Unary { op: UnaryOp::Neg | UnaryOp::Not, expr } | ExprKind::Group(expr) => constant_initializer(expr),
            _ => false,
        }
    }
}

impl Lowerer<'_> {
    pub(super) fn fixed_literal(&mut self, expr: &Expr, ty: &Type) -> Option<Rvalue> {
        let (element, _) = ty.as_fixed_array()?;
        let (op, values) = match initializer(expr)? {
            Initializer::Elements(values) => (LITERAL, values.iter().map(|value| self.lower_expr_for_owned_value(value, Some(element))).collect()),
            Initializer::Repeat(value, _) => (REPEAT, vec![self.lower_expr_with_expected(value, Some(element))]),
        };
        Some(call(op, ty, values))
    }

    pub(super) fn lower_fixed_read(&mut self, object: &Expr, index: &Expr) -> Option<Operand> {
        let ty = self.infer_expr_type(object)?;
        let (element, _) = ty.as_fixed_array()?;
        let array = self.lower_expr_at_sequence_point(object, Some(&ty));
        let offset = self.lower_index_domain_expr(index);
        let dest = self.new_typed_temp(element.clone());
        self.emit(Instruction::Assign { target: dest.clone(), value: access(READ, &ty, array, offset, None, index.span) });
        Some(Operand::Place(dest))
    }

    pub(super) fn lower_fixed_assign(&mut self, assign: &crate::ast::AssignStmt) -> bool {
        let AssignTarget::Index { object, index } = &assign.target else { return false };
        let Some(ty) = self.infer_expr_type(object) else { return false };
        let Some((element, _)) = ty.as_fixed_array() else { return false };
        let place = self.render_expr_place(object);
        let offset = self.lower_index_domain_expr(index);
        let value = if let Some(op) = assign.op {
            let current = self.new_typed_temp(element.clone());
            self.emit(Instruction::Assign { target: current.clone(), value: access(READ, &ty, Operand::Place(place.clone()), offset.clone(), None, index.span) });
            let expr = Expr {
                kind: ExprKind::Binary {
                    op, left: Box::new(Expr { kind: ExprKind::Name(current), span: assign.span }),
                    right: Box::new(assign.value.clone()),
                },
                span: assign.span,
            };
            self.lower_expr_with_expected(&expr, Some(element))
        } else {
            self.lower_expr_for_owned_value(&assign.value, Some(element))
        };
        self.emit(Instruction::Assign { target: place.clone(), value: access(WRITE, &ty, Operand::Place(place), offset, Some(value), index.span) });
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn fixed_iteration(&mut self, ty: &Type, array: Operand, binding: String, span: crate::diag::Span, dispatch: usize, body: usize, end: usize) {
        let (_, n) = ty.as_fixed_array().unwrap();
        let index = self.new_typed_temp(Type::named("int64"));
        self.emit(Instruction::Assign { target: index.clone(), value: Rvalue::Use(Operand::Int(0)) });
        self.terminate(Terminator::Goto(self.label(dispatch)));
        self.switch_to(dispatch);
        let next = self.new_block("fixed_next");
        let more = self.new_typed_temp(Type::named("bool"));
        self.emit(Instruction::Assign {target: more.clone(), value: Rvalue::Binary {
            op: BinaryOp::Less, left: Operand::Place(index.clone()), right: Operand::Int(n as u128), span, trap: ArithmeticTrap::None,
        }});
        self.terminate(Terminator::Branch {condition: Operand::Place(more), then_label: self.label(next), else_label: self.label(end)});
        self.switch_to(next);
        self.emit(Instruction::Assign { target: binding, value: access(READ, ty, array, Operand::Place(index.clone()), None, span) });
        self.emit(Instruction::Assign {target: index.clone(), value: Rvalue::Binary {
            op: BinaryOp::Add, left: Operand::Place(index), right: Operand::Int(1), span, trap: ArithmeticTrap::None,
        }});
        self.terminate(Terminator::Goto(self.label(body)));
    }
}
