//! A naked function's single-instruction body is checked after guard selection.
use super::*;

fn naked(decorators: &[Decorator]) -> bool {
    decorators.iter().any(|decorator| {
        crate::machine_attributes::attribute(decorator).is_some_and(|(name, _)| name == "naked")
    })
}

impl Parser {
    pub(in crate::parser) fn attach_machine_attributes(
        &mut self,
        function: &mut FunctionDecl,
        decorators: Vec<Decorator>,
    ) -> Result<()> {
        if naked(&decorators) && function.body.iter().any(|stmt| matches!(stmt, Stmt::If(_))) {
            self.deferred_naked = true;
            function.decorators = decorators;
            return Ok(());
        }
        crate::machine_attributes::attach(function, decorators)
    }

    pub(in crate::parser) fn finish_naked_functions(&self, module: &mut Module) -> Result<()> {
        if self.deferred_naked {
            for item in &mut module.items {
                if let Item::Function(function) = item {
                    if naked(&function.decorators) {
                        let decorators = std::mem::take(&mut function.decorators);
                        let body = std::mem::take(&mut function.body);
                        append_selected(body, &mut function.body);
                        crate::machine_attributes::attach(function, decorators)?;
                    }
                }
            }
        }
        Ok(())
    }
}

// A selected if stores its chosen suite in else_body with no runtime tests.
// Retain ordinary if statements so the naked-body diagnostic rejects them.
fn append_selected(statements: Vec<Stmt>, body: &mut Vec<Stmt>) {
    for statement in statements {
        match statement {
            Stmt::If(branch) if branch.branches.is_empty() => {
                if let Some(selected) = branch.else_body {
                    append_selected(selected, body);
                }
            }
            other => body.push(other),
        }
    }
}
