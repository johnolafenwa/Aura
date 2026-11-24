//! Editor signatures for operations that the checker handles as intrinsics.
use super::*;

const MEM_INTRINSICS: &[(&str, &str)] = &[
    ("address_of", "address_of(place: T) -> ptr[T]"),
    ("size_of", "size_of(type) -> uint64"),
    ("align_of", "align_of(type) -> uint64"),
    ("offset_of", "offset_of(type, field) -> uint64"),
];

pub(super) fn builtin_mem(namespace: &crate::sema::ModuleNamespace) -> bool {
    namespace.path == "mem" && namespace.source_path.is_none()
}

pub(super) fn mem_completions() -> impl Iterator<Item = AnalysisCompletion> {
    MEM_INTRINSICS
        .iter()
        .map(|&(name, detail)| AnalysisCompletion {
            name: name.into(),
            kind: "function".into(),
            detail: detail.into(),
            documentation: None,
        })
}

pub(super) fn mem_member(name: &str) -> Option<ResolvedMember> {
    let (_, detail) = MEM_INTRINSICS.iter().find(|(member, _)| *member == name)?;
    Some(ResolvedMember {
        hover: format!("```aura\n{detail}\n```"),
        definition: None,
        // address_of obtains its result type from the argument place.
        ty: (name != "address_of").then(|| Type::named("uint64")),
    })
}

pub(super) fn raw_pointer_signature(element: &Type, method: &str) -> Option<(String, Type)> {
    let result = crate::sema::raw_pointer_result(element, method)?;
    let parameter = crate::sema::raw_pointer_parameter(element, method)
        .map(|(name, ty)| format!("{name}: {ty}"))
        .unwrap_or_default();
    Some((format!("{method}({parameter}) -> {result}"), result))
}

impl AnalysisBuilder<'_> {
    /// In offset_of(T, field), the second operand names a field of T even
    /// when a local binding happens to have the same name.
    pub(super) fn visit_offset_field(
        &mut self,
        callee: &Expr,
        args: &[crate::ast::Argument],
        scope: &BTreeMap<String, BindingInfo>,
    ) -> bool {
        let ExprKind::Member { object, field } = &callee.kind else {
            return false;
        };
        if field != "offset_of" {
            return false;
        }
        let Some(Type::Module(path)) = self.infer_expr_type(object, scope) else {
            return false;
        };
        if !self.module_namespace(&path).is_some_and(builtin_mem) {
            return false;
        }
        let [class, member] = args else { return false };
        let ExprKind::Name(name) = &member.value.kind else {
            return false;
        };
        let Some(ty) = self.analysis_type_arg_expr(&class.value) else {
            return false;
        };
        self.visit_expr(&class.value, scope);
        if let Some(resolved) = self.resolve_member_type(&ty, name) {
            self.push_occurrence(
                range_from_span(member.value.span, name.len()),
                resolved.hover,
                resolved.definition,
            );
        }
        true
    }
}

pub(super) fn layout_hover(class: &crate::ast::ClassDecl) -> String {
    use std::fmt::Write;

    let layout = &class.layout_controls;
    let mut signature = String::from("```aura\nextern \"C\"");
    if layout.packed {
        signature.push_str(" packed");
    }
    if let Some(alignment) = layout.alignment {
        write!(signature, " align({alignment})").unwrap();
    }
    write!(signature, " class {}:", class.name).unwrap();
    for field in &class.fields {
        let ty = lower_type_ref(&field.ty);
        let display = match class.c_arrays.get(&field.name) {
            Some(dimensions) => {
                // Source array fields are represented as one list per dimension.
                let element = dimensions
                    .iter()
                    .fold(&ty, |inner, _| &inner.type_arguments()[0]);
                dimensions
                    .iter()
                    .rev()
                    .fold(element.to_string(), |inner, length| {
                        format!("array[{inner}, {length}]")
                    })
            }
            None => ty.to_string(),
        };
        write!(signature, "\n    {}: {}", field.name, display).unwrap();
        if let Some(width) = layout.bit_widths.get(&field.name) {
            write!(signature, " bits({width})").unwrap();
        }
    }
    signature.push_str("\n```");
    with_doc(signature, class.doc.as_ref())
}
