use super::*;
use crate::inline_asm::{integer_width, AsmStatement};

impl Lowerer<'_> {
    pub(super) fn lower_inline_asm(&mut self, expr: &Expr) -> bool {
        let asm = AsmStatement::parse(expr)
            .expect("checked asm")
            .expect("asm statement");
        let mut widths = Vec::new();
        let mut inputs = Vec::new();
        for (_, value) in &asm.inputs {
            let ty = self.infer_expr_type(value).expect("checked input type");
            widths.push(integer_width(&ty).expect("checked integer input"));
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
            out_widths.push(integer_width(&ty).expect("checked integer output"));
            outputs.push(self.render_local_name(name));
        }
        let target = self
            .program
            .machine_target
            .expect("assembly lowering needs a target");
        let machine = asm
            .machine(target.arch(), widths, out_widths, expr.span)
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

pub(super) fn add_link_metadata(program: &Program, plan: &mut ForeignLinkPlan) {
    plan.module_asm
        .extend_from_slice(&program.module.module_asm);
    if !plan.module_asm.is_empty() || program.module.asm_span.is_some() || plan.asm_arch.is_some() {
        plan.asm_arch = program
            .machine_target
            .map(|target| target.arch().to_owned());
        plan.asm_externs.extend(
            program
                .extern_functions
                .values()
                .map(|f| f.decl.link_name().to_owned()),
        );
        plan.asm_externs.sort();
        plan.asm_externs.dedup();
    }
}
