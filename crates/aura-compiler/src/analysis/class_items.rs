//! Source locations for class items that the checker expands.
use super::*;
use crate::ast::{ImplDecl, PROPERTY_SETTER_SUFFIX};

pub(super) fn method_source_name(name: &str) -> &str {
    name.strip_suffix(PROPERTY_SETTER_SUFFIX).unwrap_or(name)
}

impl AnalysisBuilder<'_> {
    pub(super) fn visit_delegation_field(&mut self, declaration: &ImplDecl) {
        if let Some((field, span)) = &declaration.delegation {
            let receiver = self.lower_analysis_type_ref(&declaration.for_type);
            if let Some(member) = self.resolve_member_type(&receiver, field) {
                self.push_occurrence(
                    range_from_span(*span, field.len()),
                    member.hover,
                    member.definition,
                );
            }
        }
    }

    pub(super) fn visit_class_item_declaration(
        &mut self,
        class: &ClassInfo,
        method: &FunctionDecl,
    ) {
        // The public wrapper keeps the decorators. The private body also has
        // its own declaration at the same source position.
        let name = method_source_name(&method.name);
        if name == method.name && method.decorators.is_empty() && name != "__drop__" {
            return;
        }
        let definition = self.definition_range(&class.module_name, method.span, name);
        self.push_occurrence(
            definition.clone(),
            format_method_hover(method),
            Some(definition),
        );

        if name != method.name {
            // The getter and setter have distinct declarations, even though
            // their source names coincide. The @name.setter line uses the getter.
            let preceding = &self.source_lines[..method
                .span
                .line
                .saturating_sub(1)
                .min(self.source_lines.len())];
            let marker = preceding.iter().rposition(|line| {
                let text = line.trim_start();
                !text.is_empty() && !text.starts_with('#')
            });
            if let (Some(line), Some(getter)) = (marker, class.methods.get(name)) {
                if let Some(range) = self.find_identifier_range(line + 1, name) {
                    let getter_definition =
                        self.definition_range(&class.module_name, getter.decl.span, name);
                    self.push_occurrence(
                        range,
                        format_method_hover(&getter.decl),
                        Some(getter_definition),
                    );
                }
            }
        }
    }

    pub(super) fn implementation_method_definition(
        &self,
        implementation: &crate::sema::TraitImplInfo,
        method: &FunctionDecl,
    ) -> AnalysisRange {
        if implementation.decl.delegation.is_some()
            && method.span.column >= crate::diag::GENERATED_COLUMN_STRIDE
        {
            if let Some(trait_info) = self.trait_info_named(&implementation.trait_name) {
                if let Some(original) = trait_info.methods.get(&method.name) {
                    return self.definition_range(
                        &trait_info.module_name,
                        original.decl.span,
                        &method.name,
                    );
                }
            }
        }
        self.definition_range(&implementation.module_name, method.span, &method.name)
    }
}
