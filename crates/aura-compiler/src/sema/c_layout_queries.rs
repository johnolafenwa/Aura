//! Compile-time C layout queries. The field argument names a member, so
//! neither argument is evaluated as an Aura expression.
use super::*;

pub(crate) fn evaluate(
    callee: &Expr,
    args: &[Argument],
    modules: &BTreeMap<String, ModuleNamespace>,
    classes: &BTreeMap<String, ClassInfo>,
) -> Option<Result<usize>> {
    let ExprKind::Member { object, field } = &callee.kind else {
        return None;
    };
    let ExprKind::Name(alias) = &object.kind else {
        return None;
    };
    let ns = modules.get(alias)?;
    if ns.path != "mem"
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
        let ExprKind::Name(name) = &args[0].value.kind else {
            return Err(error("expected a C type name in the layout query"));
        };
        let ty = Type::named(name);
        if let Some(scalar) = ffi_scalar(&ty) {
            return if field == "offset_of" {
                Err(error("offset_of requires a C class or union"))
            } else {
                Ok(scalar.size())
            };
        }
        let layout = ffi_struct_layout(&ty, classes)
            .ok_or_else(|| error("expected a type with a C layout"))?;
        match field.as_str() {
            "size_of" => Ok(layout.size()),
            "align_of" => Ok(layout.align()),
            _ => {
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
        ] {
            let program = check(query).unwrap();
            let mir = crate::mir::lower(&program);
            assert!(!format!("{mir:?}").contains("\"offset_of\""));
        }
    }
}
