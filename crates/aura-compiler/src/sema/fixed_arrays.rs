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
    Err(Diagnostic::coded_at(
        "AU2081",
        span,
        "fixed[T, N] needs an element type and a literal length from 1 to 1048576",
    ))
}

pub(crate) enum Initializer<'a> {
    Elements(&'a [Expr]),
    Repeat(&'a Expr, usize),
}

pub(crate) fn initializer(expr: &Expr) -> Option<Initializer<'_>> {
    match &expr.kind {
        ExprKind::List(items) => Some(Initializer::Elements(items)),
        ExprKind::Binary {
            op: BinaryOp::Mul,
            left,
            right,
        } => {
            let ExprKind::List(items) = &left.kind else {
                return None;
            };
            let [value] = items.as_slice() else {
                return None;
            };
            let ExprKind::Int(n) = right.kind else {
                return None;
            };
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
        let Some((element, length)) = ty.as_fixed_array() else {
            return Ok(None);
        };
        let Some(init) = initializer(expr) else {
            return Ok(None);
        };
        let (items, count) = match init {
            Initializer::Elements(items) => (items, items.len()),
            Initializer::Repeat(value, count) => {
                if !self.is_copy_type(element) {
                    return Err(Diagnostic::coded_at(
                        "AU3031",
                        value.span,
                        "a repeated array element must be Copy",
                    ));
                }
                (std::slice::from_ref(value), count)
            }
        };
        if length != count {
            return Err(Diagnostic::coded_at(
                "AU2082",
                expr.span,
                format!("fixed array needs {length} elements, found {count}"),
            ));
        }
        for item in items {
            let actual = self.type_of_expr_hint(item, locals, Some(element))?;
            if actual != *element {
                return Err(Diagnostic::coded_at(
                    "AU2082",
                    item.span,
                    format!("fixed array element must be `{element}`, found `{actual}`"),
                ));
            }
            self.consume_aggregate_literal_element(item, locals)?;
        }
        Ok(Some(ty.clone()))
    }
}

/// Check nominal element layouts once declarations from every module are available.
pub(super) fn validate_layouts(program: &Program) -> Result<()> {
    let arrays = program.type_definitions.fixed_arrays.borrow();
    let mut cache = BTreeMap::new();
    for (&(line, column), ty) in arrays.iter() {
        if shape(program, ty, &mut cache).is_none() {
            return Err(Diagnostic::coded_at(
                "AU2081",
                Span::new(line, column),
                format!(
                    "type `{ty}` needs an inline element and at most {} bytes",
                    i32::MAX
                ),
            ));
        }
    }
    Ok(())
}

/// A shape is a byte count and alignment. A visiting class maps to None
/// so that recursive declarations cannot expand an aggregate indefinitely.
fn shape(
    program: &Program,
    ty: &Type,
    cache: &mut BTreeMap<String, Option<(u32, u32)>>,
) -> Option<(u32, u32)> {
    if let Some((element, length)) = ty.as_fixed_array() {
        let (stride, align) = shape(program, element, cache)?;
        let bytes = stride.checked_mul(u32::try_from(length).ok()?)?;
        return (bytes <= i32::MAX as u32).then_some((bytes, align));
    }
    let Type::Named(name, args) = ty else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    let scalar: Option<u32> = match name.as_str() {
        "bool" | "int8" | "uint8" => Some(1),
        "int16" | "uint16" => Some(2),
        "int32" | "uint32" | "float32" => Some(4),
        "int64" | "uint64" | "float64" => Some(8),
        _ => None,
    };
    if let Some(width) = scalar {
        return Some((width, width));
    }
    if let Some(shape) = cache.get(name) {
        return *shape;
    }
    let class = program.classes.get(name).or_else(|| {
        super::properties::copy_class_info_from_modules(
            name,
            &program.imported_modules,
            &program.module_registry,
        )
    })?;
    cache.insert(name.clone(), None);
    let mut size: u32 = 0;
    let mut align: u32 = 1;
    for field in &class.decl.fields {
        if field.ty.indirect {
            return None;
        }
        let (bytes, natural) = shape(program, &class.fields.get(&field.name)?.ty, cache)?;
        align = align.max(natural);
        size = size.checked_next_multiple_of(natural)?.checked_add(bytes)?;
    }
    let bytes = size.checked_next_multiple_of(align)?;
    if bytes == 0 {
        return None;
    }
    cache.insert(name.clone(), Some((bytes, align)));
    Some((bytes, align))
}
