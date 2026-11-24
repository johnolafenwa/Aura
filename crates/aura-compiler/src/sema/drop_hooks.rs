//! Declaration and partial-move rules for class owner cleanup.

use super::*;
use crate::ast::ClassDecl;
use crate::diag::Span;

pub(crate) const DROP_HOOK: &str = "__drop__";

pub(super) fn validate(class: &ClassDecl, method: &FunctionDecl, result: &Type) -> Result<()> {
    if method.name != DROP_HOOK {
        return Ok(());
    }
    if class.copy {
        return Err(Diagnostic::coded_at(
            "AU2065",
            method.span,
            format!("copy class `{}` cannot declare `__drop__`", class.name),
        ));
    }
    if method.contracts.iter().any(|clause| clause.kind == crate::ast::ContractKind::Requires) {
        return Err(Diagnostic::coded_at("AU2065", method.span, "drop hook cannot require a precondition"));
    }
    if method.receiver != Some(ReceiverKind::BorrowMut)
        || !method.params.is_empty()
        || !method.type_params.is_empty()
        || method.view_return.is_some()
        || result != &Type::Unit
        || method.property.is_some()
    {
        return Err(Diagnostic::coded_at(
            "AU2065",
            method.span,
            "drop hook must be `def __drop__(mut self)` with no other parameters and a None result",
        ));
    }
    Ok(())
}

impl FunctionChecker<'_> {
    pub(super) fn check_drop_field_move(
        &self,
        object: &Expr,
        ty: &Type,
        locals: &HashMap<String, LocalBinding>,
        span: Span,
    ) -> Result<()> {
        let check_type = |ty: &Type| {
            if let Type::Named(name, _) = ty {
                if self
                    .resolve_class_info(name)
                    .is_some_and(|class| class.methods.contains_key(DROP_HOOK))
                {
                    return Err(Diagnostic::coded_at(
                        "AU3020",
                        span,
                        format!("cannot move a field out of `{ty}` because it declares `__drop__`"),
                    ));
                }
            }
            Ok(())
        };
        check_type(ty)?;
        if let Some(path) = self.member_access_path(object) {
            if let Some(root) = locals.get(&path.root) {
                let mut parent = root.ty.clone();
                for component in &path.projections.0 {
                    check_type(&parent)?;
                    let PlaceProjection::Field(field) = component else { break };
                    let Ok(next) = self.resolve_member_type(&parent, field, span) else { break };
                    parent = next;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_hook_and_diagnoses_ownership_errors() {
        for (source, code) in [
            (include_str!("../../tests/fixtures/check-fail/class_drop_copy.au"), "AU2065"),
            (include_str!("../../tests/fixtures/check-fail/class_drop_signature.au"), "AU2065"),
            (include_str!("../../tests/fixtures/check-fail/class_drop_precondition.au"), "AU2065"),
            (include_str!("../../tests/fixtures/check-fail/class_drop_explicit.au"), "AU2066"),
            (include_str!("../../tests/fixtures/check-fail/class_drop_partial_move.au"), "AU3020"),
        ] {
            let module = crate::parser::parse(source).unwrap();
            assert_eq!(check(module).unwrap_err().code, code);
        }
        check(crate::parser::parse(include_str!("../../tests/fixtures/check-pass/class_drop_hook.au")).unwrap()).unwrap();
    }
}
