use super::*;
use crate::inline_asm::{operand_width, target_at, AsmStatement};

impl FunctionChecker<'_> {
    pub(super) fn check_inline_asm(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<bool> {
        let target = target_at(expr.span)?;
        let asm = AsmStatement::parse(expr)?.expect("asm statement");
        let mut widths = Vec::new();
        let mut names = BTreeSet::new();
        for (reg, value) in &asm.inputs {
            let ty = self.type_of_expr(value, locals)?;
            widths.push(operand_width(target, reg, &ty, value.span)?);
            self.consume_direct_read_expr(value, locals)?;
        }
        let mut outputs = Vec::new();
        for (reg, value) in &asm.outputs {
            let ExprKind::Name(name) = &value.kind else {
                return Err(Diagnostic::coded_at(
                    "AU3035",
                    value.span,
                    "asm output needs a mutable scalar binding",
                ));
            };
            let Some(binding) = locals
                .get(name)
                .filter(|b| (b.assignable || b.mutable_place) && names.insert(name))
            else {
                return Err(Diagnostic::coded_at(
                    "AU3035",
                    value.span,
                    "asm output needs a distinct mutable scalar binding",
                ));
            };
            let width = operand_width(target, reg, &binding.ty, value.span)?;
            self.check_assign(
                &AssignStmt {
                    mutable: false,
                    target: AssignTarget::Name(name.clone()),
                    annotation: None,
                    op: None,
                    value: (*value).clone(),
                    span: value.span,
                },
                locals,
            )?;
            outputs.push(width);
        }
        asm.machine(target, widths, outputs, expr.span)?;
        Ok(asm.noreturn)
    }
}
