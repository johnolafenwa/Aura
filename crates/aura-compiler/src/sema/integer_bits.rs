use super::*;
use aura_core::integer_bits::BitMethod;

impl FunctionChecker<'_> {
    pub(super) fn check_bit_method(
        &self,
        method: BitMethod,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        receiver: Type,
    ) -> Result<Type> {
        let ordered = BuiltinMember::IntegerBits(method).bind_args(args, span)?;
        if let Some(Some(count)) = ordered.first() {
            let ty = Type::named("int64");
            if self.type_of_expr_hint(&count.value, locals, Some(&ty))? != ty {
                return Err(Diagnostic::coded_at(
                    "AU2088",
                    count.span,
                    "rotation count must be int64",
                ));
            }
        }
        Ok(if method.counts() {
            Type::named("int64")
        } else {
            receiver
        })
    }
}
