//! Range checks on stores to C bit-fields. The source value is evaluated
//! once, before its comparison and store.
use super::*;

impl Lowerer<'_> {
    pub(super) fn c_layout_query(&self, callee: &Expr, args: &[Argument]) -> Option<crate::diag::Result<usize>> {
        let (modules, classes) = self.current_module_namespace().map(|ns| (&ns.imported_modules, &ns.all_classes)).unwrap_or((&self.program.imported_modules, &self.program.classes));
        crate::sema::c_layout_queries::evaluate(callee, args, modules, classes, |name| self.binding_is_visible(name))
    }

    pub(super) fn check_member_bit_store(&mut self, assign: &AssignStmt, value: &Operand) {
        let AssignTarget::Member { object, field } = &assign.target else {
            return;
        };
        let Some(Type::Named(name, _)) = self.infer_expr_type(object) else {
            return;
        };
        let width = self
            .resolve_class_info(&name)
            .and_then(|class| class.decl.layout_controls.bit_widths.get(field))
            .copied();
        self.check_bit_store(width, value, assign.span);
    }

    pub(super) fn check_bit_store(&mut self, width: Option<u8>, value: &Operand, span: Span) {
        let Some(width) = width else { return };
        let max = (1u128 << width) - 1;
        if width == 64 || matches!(value, Operand::Int(n) if *n <= max) {
            return;
        }
        let condition = self.new_typed_temp(Type::named("bool"));
        self.emit(Instruction::Assign {
            target: condition.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::LessEq,
                left: value.clone(),
                right: Operand::Int(max),
                span,
                trap: ArithmeticTrap::Runtime,
            },
        });
        let fail = self.new_block("c_bit_field_range");
        let next = self.new_block("c_bit_field_fits");
        self.terminate(Terminator::Branch {
            condition: Operand::Place(condition),
            then_label: self.label(next),
            else_label: self.label(fail),
        });
        self.switch_to(fail);
        self.terminate(Terminator::AssertFail {
            message: Some(Operand::String(format!(
                "value does not fit the {width}-bit C field"
            ))),
            captures: vec![],
            span,
            kind: AssertKind::CBitField,
        });
        self.switch_to(next);
    }
}
