//! Compile-time C layout queries. The field argument names a member, so
//! neither argument is evaluated as an Aura expression.
use super::*;

impl FunctionChecker<'_> {
    pub(super) fn c_layout_query(&self, callee: &Expr, args: &[Argument], locals: &HashMap<String, LocalBinding>) -> Option<Result<usize>> {
        let (modules, classes) = self.current_module_namespace().map(|ns| (&ns.imported_modules, &ns.all_classes)).unwrap_or((self.imported_modules, self.classes));
        evaluate(callee, args, modules, classes, |name| locals.get(name).is_some_and(|binding| !matches!(binding.ty, Type::Module(_))))
    }
}

pub(crate) fn evaluate(
    callee: &Expr,
    args: &[Argument],
    modules: &BTreeMap<String, ModuleNamespace>,
    classes: &BTreeMap<String, ClassInfo>,
    shadowed: impl Fn(&str) -> bool,
) -> Option<Result<usize>> {
    let ExprKind::Member { object, field } = &callee.kind else {
        return None;
    };
    let ExprKind::Name(alias) = &object.kind else {
        return None;
    };
    let ns = modules.get(alias)?;
    if shadowed(alias)
        || ns.path != "mem"
        || ns.source_path.is_some()
        || !matches!(field.as_str(), "size_of" | "align_of" | "offset_of")
    {
        return None;
    }
    let error = |message: &str| Diagnostic::coded_at("AU2086", callee.span, message);
    Some((|| {
        if args.len() != if field == "offset_of" { 2 } else { 1 }
            || args.iter().any(|a| a.name.is_some())
        {
            return Err(error(
                "a C layout query expects a type name, with a field name for offset_of",
            ));
        }
        let kind = query_type(&args[0].value, modules, classes, &shadowed)
            .ok_or_else(|| error("expected a type with a C layout"))?;
        match field.as_str() {
            "size_of" => Ok(kind.size()),
            "align_of" => Ok(kind.align()),
            _ => {
                let crate::c_layout::CFieldKind::Struct(layout) = kind else {
                    return Err(error("offset_of requires a C class or union"));
                };
                let ExprKind::Name(member) = &args[1].value.kind else {
                    return Err(error("expected a field name in offset_of"));
                };
                if layout.layout_controls.bit_widths.contains_key(member) {
                    return Err(error("a bit-field has no addressable byte offset"));
                }
                layout
                    .fields
                    .iter()
                    .zip(layout.field_offsets())
                    .find_map(|(f, n)| (&f.name == member).then_some(n))
                    .ok_or_else(|| error("field is absent from the C layout"))
            }
        }
    })())
}

fn query_type(
    expr: &Expr,
    modules: &BTreeMap<String, ModuleNamespace>,
    classes: &BTreeMap<String, ClassInfo>,
    shadowed: &impl Fn(&str) -> bool,
) -> Option<crate::c_layout::CFieldKind> {
    use crate::c_layout::CFieldKind;
    let (name, classes) = match &expr.kind {
        ExprKind::Name(name) => (name, classes),
        ExprKind::Member { object, field } => {
            let ExprKind::Name(module) = &object.kind else { return None };
            let namespace = modules.get(module).filter(|_| !shadowed(module))?;
            namespace.classes.get(field)?;
            (field, &namespace.all_classes)
        }
        ExprKind::Index { object, index } => {
            let ExprKind::Name(name) = &object.kind else { return None };
            if name == "ptr" {
                query_type(index, modules, classes, shadowed)?;
                return Some(CFieldKind::Scalar(crate::ffi::FfiScalar::U64));
            }
            let ExprKind::Tuple(args) = &index.kind else { return None };
            let [element, length] = args.as_slice() else { return None };
            let ExprKind::Int(length @ 1..=1_048_576) = length.kind else { return None };
            if name != "fixed" { return None; }
            let element = query_type(element, modules, classes, shadowed)?;
            let count = length as usize;
            element.size().checked_mul(count)?;
            return Some(CFieldKind::Array(Box::new(element), count));
        }
        _ => return None,
    };
    let ty = Type::named(name);
    ffi_scalar(&ty).map(CFieldKind::Scalar).or_else(|| ffi_struct_layout(&ty, classes).map(CFieldKind::Struct))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(query: &str) -> Result<Program> {
        let source = format!("import mem as m\nextern \"C\" packed class S:\n    code: uint8 bits(3)\n    value: uint32\n\ndef run() -> uint64:\n    return {query}\n");
        crate::check_module_with_builtin_imports(crate::parse_source(&source)?)
    }

    #[test]
    fn query_requires_c_type_and_addressable_field() {
        for query in [
            "m.size_of()",
            "m.align_of(value=uint8)",
            "m.size_of(1)",
            "m.size_of(fixed[uint32, 0])",
            "m.size_of(ptr[Missing])",
            "m.size_of(fixed[uint16])",
            "m.size_of(list[uint16, 2])",
            "m.size_of(fixed[fixed[fixed[fixed[uint8, 1048576], 1048576], 1048576], 1048576])",
            "m.size_of(list)",
            "m.offset_of(uint32, x)",
            "m.offset_of(S, code)",
            "m.offset_of(S, missing)",
            "m.offset_of(S, 0)",
        ] {
            assert_eq!(check(query).unwrap_err().code, "AU2086", "{query}");
        }
        for query in [
            "m.size_of(S)",
            "m.align_of(S)",
            "m.offset_of(S, value)",
            "m.align_of(uint64)",
            "m.size_of(fixed[S, 3])",
            "m.align_of(ptr[S])",
        ] {
            let program = check(query).unwrap();
            let mir = crate::mir::lower(&program);
            assert!(!format!("{mir:?}").contains("\"offset_of\""));
        }
    }
}

/// Fold a complete query in a module initializer after its original type
/// has been checked. This gives freestanding storage an integer literal.
pub(super) fn folded_binding(
    decl: &crate::ast::ConstantDecl,
    modules: &BTreeMap<String, ModuleNamespace>,
    classes: &BTreeMap<String, ClassInfo>,
) -> crate::ast::ConstantDecl {
    let mut decl = decl.clone();
    if let ExprKind::Call { callee, args } = &decl.value.kind {
        if let Some(Ok(value)) = evaluate(callee, args, modules, classes, |_| false) {
            decl.value.kind = ExprKind::Int(value as u128);
        }
    }
    decl
}
