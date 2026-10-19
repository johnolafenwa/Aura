//! Builtin modules with an Aura-source part.
//!
//! Most builtin module members are host functions and enums declared in
//! `builtin_modules`. A module listed here also has public items written in
//! Aura under `std/`. The compiler checks that source as the module itself,
//! with the host part imported under the module's own name, and merges its
//! exports into the namespace that `import` binds. Its functions, classes,
//! and implementations then lower like any imported module's.

use crate::ast::ImportKind;
use crate::builtin_modules::builtin_module_namespace;
use crate::sema::{ModuleContext, ModuleNamespace};
use std::cell::RefCell;
use std::collections::BTreeMap;

const JSON_SOURCE: &str = include_str!("../std/json.au");

fn module_source(name: &str) -> Option<&'static str> {
    match name {
        "json" => Some(JSON_SOURCE),
        _ => None,
    }
}

/// Whether the builtin module at `path` has an Aura-source part.
pub(crate) fn has_source_part(path: &[String]) -> bool {
    matches!(path, [name] if module_source(name).is_some())
}

/// Whether `module` imports a builtin module that has an Aura-source part.
pub(crate) fn imports_source_part(module: &crate::ast::Module) -> bool {
    module.imports.iter().any(|import| match &import.kind {
        ImportKind::Module { path, .. } => has_source_part(path),
        ImportKind::From { module_path, .. } => has_source_part(module_path),
    })
}

thread_local! {
    static CHECKED: RefCell<BTreeMap<String, ModuleNamespace>> = const { RefCell::new(BTreeMap::new()) };
}

/// The exports of the Aura-source part of builtin module `name`.
fn source_part(name: &str) -> Option<ModuleNamespace> {
    let source = module_source(name)?;
    if let Some(namespace) = CHECKED.with(|checked| checked.borrow().get(name).cloned()) {
        return Some(namespace);
    }
    let module = crate::parse_source(source)
        .unwrap_or_else(|error| panic!("builtin `{name}` source should parse: {error:?}"));
    let imported_bindings = crate::builtin_imports_with(&module, false)
        .unwrap_or_else(|error| panic!("builtin `{name}` imports should resolve: {error:?}"));
    let program = crate::sema::check_with_context(
        module,
        ModuleContext {
            module_name: name.to_string(),
            imported_bindings,
            module_registry: crate::builtin_modules::builtin_module_registry(),
            is_entry_module: false,
        },
    )
    .unwrap_or_else(|error| panic!("builtin `{name}` source should check: {error:?}"));
    let namespace = crate::exported_namespace(&[name.to_string()], &program);
    CHECKED.with(|checked| {
        checked
            .borrow_mut()
            .insert(name.to_string(), namespace.clone())
    });
    Some(namespace)
}

/// The namespace `import` binds for builtin module `path`: the host part,
/// plus the Aura-source part when the module has one.
pub(crate) fn importable_builtin_namespace(path: &[String]) -> Option<ModuleNamespace> {
    let mut namespace = builtin_module_namespace(path)?;
    if let [name] = path {
        if let Some(part) = source_part(name) {
            merge(&mut namespace, part);
        }
    }
    Some(namespace)
}

fn merge(host: &mut ModuleNamespace, part: ModuleNamespace) {
    host.union_injections.extend(part.union_injections);
    host.narrowed_reads.extend(part.narrowed_reads);
    host.type_param_calls.extend(part.type_param_calls);
    host.functions.extend(part.functions);
    host.all_functions.extend(part.all_functions);
    host.classes.extend(part.classes);
    host.all_classes.extend(part.all_classes);
    host.enums.extend(part.enums);
    host.all_enums.extend(part.all_enums);
    host.traits.extend(part.traits);
    host.all_traits.extend(part.all_traits);
    host.trait_impls.extend(part.trait_impls);
    host.imported_modules.extend(part.imported_modules);
    host.closures.extend(part.closures);
    host.comprehensions.extend(part.comprehensions);
}

/// The builtin module registry, with the Aura-source parts merged in when
/// `with_source_parts` is set. Programs that import no such module keep the
/// host-only registry, so their output carries none of these items.
pub(crate) fn builtin_registry(with_source_parts: bool) -> BTreeMap<String, ModuleNamespace> {
    let mut registry = crate::builtin_modules::builtin_module_registry();
    if with_source_parts {
        for (name, namespace) in registry.iter_mut() {
            if let Some(part) = source_part(name) {
                merge(namespace, part);
            }
        }
    }
    registry
}

#[cfg(test)]
#[path = "builtin_sources_tests.rs"]
mod tests;
