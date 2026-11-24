//! Inline array operations use a typed instruction call. Each retains its
//! array type so an interpreter and a cross compiler agree on the length.
use super::*;
use crate::sema::fixed_arrays::{initializer, Initializer};

pub(crate) const LITERAL: &str = "#fixed.literal";
pub(crate) const REPEAT: &str = "#fixed.repeat";
pub(crate) const READ: &str = "#fixed.read";
pub(crate) const ADDRESS: &str = "#fixed.address";
pub(crate) const WRITE: &str = "#fixed.write";

pub(crate) fn is_operation(name: &str) -> bool {
    matches!(name, LITERAL | REPEAT | READ | WRITE | ADDRESS)
}

pub(super) fn call(op: &str, ty: &Type, values: Vec<Operand>) -> Rvalue {
    Rvalue::Call {
        callee: CallTarget::Name(op.to_owned()),
        args: values
            .into_iter()
            .map(|value| MirArg {
                name: None,
                value,
                writeback_place: None,
            })
            .collect(),
        type_args: BTreeMap::from([("array".to_string(), ty.clone())]),
    }
}

fn access(
    op: &str,
    ty: &Type,
    object: Operand,
    index: Operand,
    value: Option<Operand>,
    span: crate::diag::Span,
) -> Rvalue {
    let mut args = vec![
        object,
        index,
        Operand::Int(span.line as u128),
        Operand::Int(span.column as u128),
    ];
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
            ExprKind::Unary {
                op: UnaryOp::Neg | UnaryOp::Not,
                expr,
            }
            | ExprKind::Group(expr) => constant_initializer(expr),
            _ => false,
        },
    }
}

/// An indexed projection that must be written back after a nested mutation.
struct ArrayProjection {
    parent: String,
    child: String,
    array: Type,
    offset: Operand,
    span: crate::diag::Span,
}

impl Lowerer<'_> {
    pub(super) fn fixed_literal(&mut self, expr: &Expr, ty: &Type) -> Option<Rvalue> {
        let (element, _) = ty.as_fixed_array()?;
        let (op, values) = match initializer(expr)? {
            Initializer::Elements(values) => (
                LITERAL,
                values
                    .iter()
                    .map(|value| self.lower_expr_for_owned_value(value, Some(element)))
                    .collect(),
            ),
            Initializer::Repeat(value, _) => (
                REPEAT,
                vec![self.lower_expr_with_expected(value, Some(element))],
            ),
        };
        Some(call(op, ty, values))
    }

    pub(super) fn lower_fixed_read(&mut self, object: &Expr, index: &Expr) -> Option<Operand> {
        let ty = self.infer_expr_type(object)?;
        let (element, _) = ty.as_fixed_array()?;
        let array = self.lower_expr_at_sequence_point(object, Some(&ty));
        let offset = self.lower_index_domain_expr(index);
        let dest = self.new_typed_temp(element.clone());
        self.emit(Instruction::Assign {
            target: dest.clone(),
            value: access(READ, &ty, array, offset, None, index.span),
        });
        Some(Operand::Place(dest))
    }

    pub(super) fn lower_fixed_assign(&mut self, assign: &crate::ast::AssignStmt) -> bool {
        let (object, index, field) = match &assign.target {
            AssignTarget::Index { object, index } => (&**object, Some(&**index), None),
            AssignTarget::Member { object, field } => (&**object, None, Some(field)),
            _ => return false,
        };
        let array_type = self.infer_expr_type(object);
        if index.is_some() && array_type.as_ref().and_then(Type::as_fixed_array).is_none() {
            return false;
        }
        if field.is_some() && !self.contains_fixed_index(object) {
            return false;
        }
        let mut projections = Vec::new();
        let Some(place) = self.fixed_mutation_place(object, &mut projections) else {
            return false;
        };
        if let Some(field) = field {
            let mut assignment = assign.clone();
            assignment.target = AssignTarget::Member {
                object: Box::new(Expr {
                    kind: ExprKind::Name(place),
                    span: object.span,
                }),
                field: field.clone(),
            };
            self.lower_assign(&assignment);
        } else if let (Some(ty), Some(index)) = (array_type, index) {
            self.assign_fixed_element(assign, place, &ty, index);
        }
        for projection in projections.into_iter().rev() {
            self.emit(Instruction::Assign {
                target: projection.parent.clone(),
                value: access(
                    WRITE,
                    &projection.array,
                    Operand::Place(projection.parent),
                    projection.offset,
                    Some(Operand::Place(projection.child)),
                    projection.span,
                ),
            });
        }
        true
    }

    fn contains_fixed_index(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member { object, .. } | ExprKind::Group(object) => {
                self.contains_fixed_index(object)
            }
            ExprKind::Index { object, .. } => self
                .infer_expr_type(object)
                .as_ref()
                .and_then(Type::as_fixed_array)
                .is_some(),
            _ => false,
        }
    }

    fn fixed_mutation_place(
        &mut self,
        expr: &Expr,
        projections: &mut Vec<ArrayProjection>,
    ) -> Option<String> {
        match &expr.kind {
            ExprKind::Group(inner) => self.fixed_mutation_place(inner, projections),
            ExprKind::Member { object, field } if self.contains_fixed_index(object) => {
                Some(format!(
                    "{}.{field}",
                    self.fixed_mutation_place(object, projections)?
                ))
            }
            ExprKind::Index { object, index } if self.contains_fixed_index(expr) => {
                let array = self.infer_expr_type(object)?;
                let (element, _) = array.as_fixed_array()?;
                let parent = self.fixed_mutation_place(object, projections)?;
                let offset = self.lower_index_domain_expr(index);
                let child = self.new_typed_temp(element.clone());
                self.emit(Instruction::Assign {
                    target: child.clone(),
                    value: access(
                        READ,
                        &array,
                        Operand::Place(parent.clone()),
                        offset.clone(),
                        None,
                        index.span,
                    ),
                });
                projections.push(ArrayProjection {
                    parent,
                    child: child.clone(),
                    array,
                    offset,
                    span: index.span,
                });
                Some(child)
            }
            _ => self.render_place_expr_option(expr),
        }
    }

    fn assign_fixed_element(
        &mut self,
        assign: &crate::ast::AssignStmt,
        place: String,
        ty: &Type,
        index: &Expr,
    ) {
        let (element, _) = ty
            .as_fixed_array()
            .expect("checked fixed assignment target");
        let offset = self.lower_index_domain_expr(index);
        let value = if let Some(op) = assign.op {
            let current = self.new_typed_temp(element.clone());
            self.emit(Instruction::Assign {
                target: current.clone(),
                value: access(
                    READ,
                    ty,
                    Operand::Place(place.clone()),
                    offset.clone(),
                    None,
                    index.span,
                ),
            });
            let expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(Expr {
                        kind: ExprKind::Name(current),
                        span: assign.span,
                    }),
                    right: Box::new(assign.value.clone()),
                },
                span: assign.span,
            };
            self.lower_expr_with_expected(&expr, Some(element))
        } else {
            self.lower_expr_for_owned_value(&assign.value, Some(element))
        };
        self.emit(Instruction::Assign {
            target: place.clone(),
            value: access(
                WRITE,
                ty,
                Operand::Place(place),
                offset,
                Some(value),
                index.span,
            ),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn fixed_iteration(
        &mut self,
        ty: &Type,
        array: Operand,
        binding: String,
        span: crate::diag::Span,
        dispatch: usize,
        body: usize,
        end: usize,
    ) {
        let (_, n) = ty.as_fixed_array().unwrap();
        let index = self.new_typed_temp(Type::named("int64"));
        self.emit(Instruction::Assign {
            target: index.clone(),
            value: Rvalue::Use(Operand::Int(0)),
        });
        self.terminate(Terminator::Goto(self.label(dispatch)));
        self.switch_to(dispatch);
        let next = self.new_block("fixed_next");
        let more = self.new_typed_temp(Type::named("bool"));
        self.emit(Instruction::Assign {
            target: more.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Less,
                left: Operand::Place(index.clone()),
                right: Operand::Int(n as u128),
                span,
                trap: ArithmeticTrap::None,
            },
        });
        self.terminate(Terminator::Branch {
            condition: Operand::Place(more),
            then_label: self.label(next),
            else_label: self.label(end),
        });
        self.switch_to(next);
        self.emit(Instruction::Assign {
            target: binding,
            value: access(READ, ty, array, Operand::Place(index.clone()), None, span),
        });
        self.emit(Instruction::Assign {
            target: index.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Add,
                left: Operand::Place(index),
                right: Operand::Int(1),
                span,
                trap: ArithmeticTrap::None,
            },
        });
        self.terminate(Terminator::Goto(self.label(body)));
    }
}
