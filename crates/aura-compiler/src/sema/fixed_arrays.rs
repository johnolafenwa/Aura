//! Type and literal rules for contiguous inline arrays.
use super::*;
use crate::ast::TypeRefKind;
use crate::diag::Span;

pub(super) fn type_parts(args: &[TypeRef], span: Span) -> Result<(&TypeRef, usize)> {
    if let [element, length] = args {
        if let TypeRefKind::Named { name, args } = &length.kind {
            if args.is_empty() {
                if let Ok(count @ 1..=1_048_576) = name.parse::<usize>() {
                    return Ok((element, count));
                }
            }
        }
    }
    Err(Diagnostic::coded_at("AU2081", span, "fixed[T, N] needs an element type and a literal length from 1 to 1048576"))
}

pub(crate) enum Initializer<'a> {
    Elements(&'a [Expr]),
    Repeat(&'a Expr, usize),
}

pub(crate) fn initializer(expr: &Expr) -> Option<Initializer<'_>> {
    match &expr.kind {
        ExprKind::List(items) => Some(Initializer::Elements(items)),
        ExprKind::Binary { op: BinaryOp::Mul, left, right } => {
            let ExprKind::List(items) = &left.kind else { return None };
            let [value] = items.as_slice() else { return None };
            let ExprKind::Int(n) = right.kind else { return None };
            Some(Initializer::Repeat(value, usize::try_from(n).ok()?))
        }
        _ => None,
    }
}

impl FunctionChecker<'_> {
    pub(super) fn fixed_literal_type(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Option<Type>> {
        let Some(ty) = expected else { return Ok(None) };
        let Some((element, length)) = ty.as_fixed_array() else { return Ok(None) };
        let Some(init) = initializer(expr) else { return Ok(None) };
        let (items, count) = match init {
            Initializer::Elements(items) => (items, items.len()),
            Initializer::Repeat(value, count) => {
                if !self.is_copy_type(element) {
                    return Err(Diagnostic::coded_at("AU3031", value.span, "a repeated array element must be Copy"));
                }
                (std::slice::from_ref(value), count)
            }
        };
        if length != count {
            return Err(Diagnostic::coded_at("AU2082", expr.span, format!("fixed array needs {length} elements, found {count}")));
        }
        for item in items {
            let actual = self.type_of_expr_hint(item, locals, Some(element))?;
            if actual != *element {
                return Err(Diagnostic::coded_at("AU2082", item.span, format!("fixed array element must be `{element}`, found `{actual}`")));
            }
            self.consume_aggregate_literal_element(item, locals)?;
        }
        Ok(Some(ty.clone()))
    }
}
