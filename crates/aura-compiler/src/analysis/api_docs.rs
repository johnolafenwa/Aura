//! Generated API documentation for one checked module.
//!
//! The data comes from the checked module's public declarations. Docstrings
//! are presented with the shared normalization; nothing in documentation runs.

use super::*;

/// The documentation of one module's public interface.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApiModule {
    pub schema_version: u32,
    pub module: String,
    pub doc: Option<String>,
    pub declarations: Vec<ApiDeclaration>,
}

/// One public declaration and its documented members.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApiDeclaration {
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub doc: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ApiMember>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ApiMember>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<ApiMember>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<ApiDeclaration>,
}

/// A field, parameter, or variant with its signature and documentation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApiMember {
    pub name: String,
    pub signature: String,
    pub doc: Option<String>,
}

/// The JSON schema version of [`ApiModule`].
pub const API_DOCS_SCHEMA_VERSION: u32 = 1;

fn normalized(doc: Option<&DocString>) -> Option<String> {
    doc.map(|doc| crate::ast::normalize_docstring(&doc.text))
        .filter(|text| !text.trim().is_empty())
}

fn declaration(
    kind: &str,
    name: &str,
    signature: String,
    doc: Option<&DocString>,
) -> ApiDeclaration {
    ApiDeclaration {
        kind: kind.to_string(),
        name: name.to_string(),
        signature,
        doc: normalized(doc),
        parameters: Vec::new(),
        fields: Vec::new(),
        variants: Vec::new(),
        methods: Vec::new(),
    }
}

fn type_params(names: &[String], bounds: &BTreeMap<String, Vec<TypeRef>>) -> String {
    if names.is_empty() {
        return String::new();
    }
    let rendered = names
        .iter()
        .map(|name| match bounds.get(name) {
            Some(bounds) if !bounds.is_empty() => format!(
                "{name}: {}",
                bounds
                    .iter()
                    .map(|bound| lower_type_ref(bound).to_string())
                    .collect::<Vec<_>>()
                    .join(" + ")
            ),
            _ => name.clone(),
        })
        .collect::<Vec<_>>();
    format!("[{}]", rendered.join(", "))
}

fn function_signature(function: &FunctionDecl) -> String {
    let mut params = function
        .receiver
        .map(|receiver| canonical_receiver_spelling(receiver).to_string())
        .into_iter()
        .collect::<Vec<_>>();
    let mut keyword_only = false;
    for param in &function.params {
        if param.keyword_only && !keyword_only {
            keyword_only = true;
            params.push("*".to_string());
        }
        params.push(format_param_decl(param));
    }
    format!(
        "def {}{}({}) -> {}",
        function.name,
        type_params(&function.type_params, &function.type_param_bounds),
        params.join(", "),
        format_decl_return(function)
    )
}

fn function_declaration(kind: &str, function: &FunctionDecl) -> ApiDeclaration {
    let mut entry = declaration(
        kind,
        &function.name,
        function_signature(function),
        function.doc.as_ref(),
    );
    let args = function
        .doc
        .as_ref()
        .map(|doc| crate::ast::docstring_args(&doc.text))
        .unwrap_or_default();
    entry.parameters = function
        .params
        .iter()
        .map(|param| ApiMember {
            name: param.name.clone(),
            signature: format_param_decl(param),
            doc: args
                .iter()
                .find(|(name, _)| *name == param.name)
                .map(|(_, description)| description.clone()),
        })
        .collect();
    entry
}

/// Collects the public declarations of a checked module, in source order.
pub fn api_module_from_program(module_name: &str, program: &Program) -> ApiModule {
    let module = &program.module;
    let mut declarations = Vec::new();
    // A checked program records every declared constant and alias.
    for constant in module.constants.iter().filter(|constant| constant.public) {
        declarations.push(declaration(
            "constant",
            &constant.name,
            format!(
                "{}: {}",
                constant.name, program.constants[&constant.name].ty
            ),
            None,
        ));
    }
    for item in &module.items {
        match item {
            Item::TypeAlias(alias) if alias.public => {
                declarations.push(declaration(
                    "type",
                    &alias.name,
                    format!(
                        "type {}",
                        format_alias_signature(&program.aliases[&alias.name])
                    ),
                    None,
                ));
            }
            Item::Class(class) if class.public => {
                let mut entry = declaration(
                    "class",
                    &class.name,
                    format!(
                        "class {}{}",
                        class.name,
                        type_params(&class.type_params, &class.type_param_bounds)
                    ),
                    class.doc.as_ref(),
                );
                entry.fields = class
                    .fields
                    .iter()
                    .filter(|field| field.public)
                    .map(|field| ApiMember {
                        name: field.name.clone(),
                        signature: format!(
                            "{}: {}{}",
                            field.name,
                            lower_type_ref(&field.ty),
                            if field.default.is_some() {
                                " = ..."
                            } else {
                                ""
                            }
                        ),
                        doc: normalized(field.doc.as_ref()),
                    })
                    .collect();
                entry.methods = class
                    .methods
                    .iter()
                    .filter(|method| method.public)
                    .map(|method| function_declaration("method", method))
                    .collect();
                declarations.push(entry);
            }
            Item::Enum(enumeration) if enumeration.public => {
                let mut entry = declaration(
                    "enum",
                    &enumeration.name,
                    format!(
                        "enum {}{}",
                        enumeration.name,
                        type_params(&enumeration.type_params, &enumeration.type_param_bounds)
                    ),
                    enumeration.doc.as_ref(),
                );
                entry.variants = enumeration
                    .variants
                    .iter()
                    .map(|variant| {
                        let payloads = variant
                            .payloads
                            .iter()
                            .map(|payload| match &payload.name {
                                Some(name) => format!("{name}: {}", lower_type_ref(&payload.ty)),
                                None => lower_type_ref(&payload.ty).to_string(),
                            })
                            .collect::<Vec<_>>();
                        ApiMember {
                            name: variant.name.clone(),
                            signature: if payloads.is_empty() {
                                variant.name.clone()
                            } else {
                                format!("{}({})", variant.name, payloads.join(", "))
                            },
                            doc: None,
                        }
                    })
                    .collect();
                declarations.push(entry);
            }
            Item::Trait(trait_decl) if trait_decl.public => {
                let supertraits = trait_decl
                    .supertraits
                    .iter()
                    .map(|bound| lower_type_ref(bound).to_string())
                    .collect::<Vec<_>>();
                let mut entry = declaration(
                    "trait",
                    &trait_decl.name,
                    format!(
                        "trait {}{}{}",
                        trait_decl.name,
                        type_params(&trait_decl.type_params, &BTreeMap::new()),
                        if supertraits.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", supertraits.join(", "))
                        }
                    ),
                    trait_decl.doc.as_ref(),
                );
                entry.methods = trait_decl
                    .methods
                    .iter()
                    .map(|method| function_declaration("method", method))
                    .collect();
                declarations.push(entry);
            }
            Item::Function(function) if function.public => {
                declarations.push(function_declaration("function", function));
            }
            Item::ExternFunction(function) if function.public => {
                declarations.push(declaration(
                    "extern function",
                    &function.name,
                    format!(
                        "extern \"{}\" def {}({}) -> {}",
                        function.abi,
                        function.name,
                        function
                            .params
                            .iter()
                            .map(format_param_decl)
                            .collect::<Vec<_>>()
                            .join(", "),
                        lower_type_ref(&function.return_type)
                    ),
                    None,
                ));
            }
            Item::ExternOpaqueClass(handle) if handle.public => {
                declarations.push(declaration(
                    "extern class",
                    &handle.name,
                    format!("extern \"{}\" opaque class {}", handle.abi, handle.name),
                    None,
                ));
            }
            _ => {}
        }
    }
    ApiModule {
        schema_version: API_DOCS_SCHEMA_VERSION,
        module: module_name.to_string(),
        doc: normalized(module.doc.as_ref()),
        declarations,
    }
}

fn push_doc(output: &mut String, doc: Option<&String>) {
    if let Some(doc) = doc {
        output.push('\n');
        output.push_str(&sanitize_doc_markdown(doc));
        output.push('\n');
    }
}

fn push_members(output: &mut String, heading: &str, members: &[ApiMember]) {
    if members.is_empty() {
        return;
    }
    output.push_str(&format!("\n{heading}\n\n"));
    for member in members {
        output.push_str(&format!("- `{}`", member.signature));
        if let Some(doc) = &member.doc {
            let doc = sanitize_doc_markdown(doc).replace('\n', "\n  ");
            output.push_str(&format!(": {doc}"));
        }
        output.push('\n');
    }
}

fn push_declaration(output: &mut String, entry: &ApiDeclaration, level: &str) {
    output.push_str(&format!(
        "\n{level} {} `{}`\n\n```aura\n{}\n```\n",
        capitalize(&entry.kind),
        entry.name,
        entry.signature
    ));
    // A documented parameter list replaces the prose `Args:` section.
    let doc = if entry
        .parameters
        .iter()
        .any(|parameter| parameter.doc.is_some())
    {
        entry.doc.as_deref().map(crate::ast::docstring_without_args)
    } else {
        entry.doc.clone()
    };
    push_doc(output, doc.as_ref());
    push_members(output, "Parameters:", &entry.parameters);
    push_members(output, "Fields:", &entry.fields);
    push_members(output, "Variants:", &entry.variants);
    for method in &entry.methods {
        push_declaration(output, method, "###");
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Renders one module page as Markdown.
pub fn render_api_markdown(module: &ApiModule) -> String {
    let mut output = format!("# Module `{}`\n", module.module);
    push_doc(&mut output, module.doc.as_ref());
    if module.declarations.is_empty() {
        output.push_str("\nThis module has no public declarations.\n");
    }
    for entry in &module.declarations {
        push_declaration(&mut output, entry, "##");
    }
    output
}

#[cfg(test)]
#[path = "api_docs_tests.rs"]
mod tests;
