//! The builtin prelude (ADR-0069): traits and implementations, written in
//! Aura in `std/prelude.au`, that every module can name without an import.
//!
//! A module that declares or imports its own item of the same name keeps
//! it. Otherwise a check that fails for want of a prelude name is retried
//! with `from prelude import <name>` added, so a program that never names
//! one carries none of the prelude's items.

use super::{Diagnostic, ModuleContext, Result};
use crate::ast::{ImportDecl, ImportKind, ImportName, Item, Module};

/// The prelude's module name.
pub(crate) const PRELUDE_MODULE_NAME: &str = "prelude";

/// The prelude traits a module names directly.
const PRELUDE_TRAITS: &[&str] = &["Ord", "Add", "From", "Transfer"];

/// The trait that gives `Result` its methods, and the methods.
const RESULT_METHODS_TRAIT: &str = "ResultMethods";
const RESULT_METHODS: &[&str] = &["is_ok", "is_err", "unwrap_or", "map", "map_err"];

/// The prelude name whose absence made the check fail with `error`, when the
/// module may import it.
pub(super) fn missing_name(
    error: &Diagnostic,
    module: &Module,
    context: &ModuleContext,
) -> Option<&'static str> {
    if context.module_name == PRELUDE_MODULE_NAME {
        return None;
    }
    let message = error.message.as_str();
    let name = PRELUDE_TRAITS
        .iter()
        .copied()
        .find(|name| {
            // An unknown name is the module's own use; an unimplemented
            // `prelude.` trait is a bound from another module, whose builtin
            // implementations this module sees once it imports the trait.
            message == format!("unknown trait `{name}`")
                || message == format!("`dyn` needs a trait, and `{name}` is not a trait")
                || (message.starts_with("type `")
                    && message.contains(&format!("does not implement trait `prelude.{name}[")))
        })
        .or_else(|| {
            RESULT_METHODS
                .iter()
                .any(|method| {
                    message.starts_with(&format!("unsupported method call `{method}` on `Result["))
                })
                .then_some(RESULT_METHODS_TRAIT)
        })?;
    let declared = module
        .items
        .iter()
        .any(|item| item_name(item) == Some(name));
    let imported = context.imported_bindings.contains_key(name)
        || module.imports.iter().any(|import| match &import.kind {
            ImportKind::From { names, .. } => names
                .iter()
                .any(|imported| imported.alias.as_deref().unwrap_or(&imported.name) == name),
            ImportKind::Module { .. } => false,
        });
    (!declared && !imported).then_some(name)
}

fn item_name(item: &Item) -> Option<&str> {
    match item {
        Item::Trait(decl) => Some(&decl.name),
        Item::Class(decl) => Some(&decl.name),
        Item::Enum(decl) => Some(&decl.name),
        Item::Function(decl) => Some(&decl.name),
        _ => None,
    }
}

/// Adds `from prelude import <name>` to `module` and binds it in `context`.
pub(super) fn import_name(
    module: &mut Module,
    context: &mut ModuleContext,
    name: &str,
) -> Result<()> {
    let path = vec![PRELUDE_MODULE_NAME.to_string()];
    let span = crate::diag::Span::new(1, 1);
    let binding = crate::builtin_modules::builtin_imported_binding(&path, name, span)?;
    context.imported_bindings.insert(name.to_string(), binding);
    if let Some(namespace) = crate::builtin_sources::importable_builtin_namespace(&path) {
        context
            .module_registry
            .insert(PRELUDE_MODULE_NAME.to_string(), namespace);
    }
    module.imports.push(ImportDecl {
        kind: ImportKind::From {
            module_path: path,
            names: vec![ImportName {
                name: name.to_string(),
                alias: None,
                span,
            }],
        },
        span,
    });
    Ok(())
}
