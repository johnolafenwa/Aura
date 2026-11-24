use super::*;
use crate::inline_asm::{integer_width, target_at, AsmStatement};

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
        for (_, value) in &asm.inputs {
            let ty = self.type_of_expr(value, locals)?;
            widths.push(integer_width(&ty).ok_or_else(|| {
                Diagnostic::coded_at(
                    "AU2091",
                    value.span,
                    format!("asm register inputs need a fixed-width integer, found `{ty}`"),
                )
            })?);
            self.consume_direct_read_expr(value, locals)?;
        }
        let mut outputs = Vec::new();
        for (_, value) in &asm.outputs {
            let ExprKind::Name(name) = &value.kind else {
                return Err(Diagnostic::coded_at(
                    "AU3035",
                    value.span,
                    "asm output needs a mutable integer binding",
                ));
            };
            let width = locals
                .get(name)
                .filter(|b| b.assignable && b.view.is_none())
                .and_then(|b| integer_width(&b.ty));
            let Some(width) = width.filter(|_| names.insert(name)) else {
                return Err(Diagnostic::coded_at(
                    "AU3035",
                    value.span,
                    "asm output needs a distinct mutable integer binding",
                ));
            };
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
        asm.machine(target.arch(), widths, outputs, expr.span)?;
        Ok(asm.noreturn)
    }
}
