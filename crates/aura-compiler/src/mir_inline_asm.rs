use super::*;
use crate::inline_asm::{scalar_width, AsmStatement};

impl Lowerer<'_> {
    pub(super) fn lower_inline_asm(&mut self, expr: &Expr) -> bool {
        let asm = AsmStatement::parse(expr)
            .expect("checked asm")
            .expect("asm statement");
        let mut widths = Vec::new();
        let mut inputs = Vec::new();
        for (_, value) in &asm.inputs {
            let ty = self.infer_expr_type(value).expect("checked input type");
            widths.push(scalar_width(&ty).expect("checked scalar input"));
            // Snapshot every input before later input expressions can mutate it.
            let input = self.lower_expr_for_owned_value(value, Some(&ty));
            let slot = self.new_typed_temp(ty);
            self.emit(Instruction::Assign {
                target: slot.clone(),
                value: Rvalue::Use(input),
            });
            inputs.push(Operand::Place(slot));
        }
        let mut outputs = Vec::new();
        let mut out_widths = Vec::new();
        for (_, value) in &asm.outputs {
            let ExprKind::Name(name) = &value.kind else {
                unreachable!("checked binding")
            };
            let ty = self.infer_expr_type(value).expect("checked output type");
            out_widths.push(scalar_width(&ty).expect("checked scalar output"));
            outputs.push(self.render_local_name(name));
        }
        let target = self
            .program
            .machine_target
            .expect("assembly lowering needs a target");
        let machine = asm
            .machine(target, widths, out_widths, expr.span)
            .expect("checked constraints");
        self.emit(Instruction::InlineAsm {
            asm: machine,
            inputs,
            outputs,
            span: expr.span,
        });
        if asm.noreturn {
            self.terminate(Terminator::Unreachable);
        }
        !asm.noreturn
    }
}
