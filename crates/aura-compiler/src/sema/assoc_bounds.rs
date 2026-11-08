//! Bounds that name a trait with associated types (Batch 9 step 9e-2).
//!
//! A trait's associated types are trailing hidden type parameters spelled
//! `Self.Name` (step 9e). A bound writes only the trait's own arguments, as
//! in `S: IntoIterator[T]`, so before checking this pass gives each such
//! bound the missing arguments: a hidden type parameter `S.Name` of the
//! function, bounded by what the trait requires of `Name`. The bound becomes
//! `S: IntoIterator[T, S.Iter]` with `S.Iter: Iterator[T]`, and `S.Iter` may
//! be written in the function's signature and body. A call infers `S.Iter`
//! from the implementation `S` resolves to.

use super::{BTreeMap, ImportedBinding, Item, Module, ModuleContext, ModuleNamespace, TraitInfo};
use crate::ast::{FunctionDecl, TraitDecl, TypeRef, TypeRefKind};

/// What a bound needs to know about a trait: its written type parameters
/// and its associated types with their bounds.
#[derive(Clone)]
struct AssocTrait {
    /// The declaring module of an imported trait. Its associated-type
    /// bounds name traits of that module, so they are qualified with it.
    module: Option<String>,
    written_params: Vec<String>,
    assoc: Vec<(String, Vec<TypeRef>)>,
}

impl AssocTrait {
    fn from_decl(decl: &TraitDecl, module: Option<&str>) -> Option<Self> {
        if decl.assoc_types.is_empty() {
            return None;
        }
        let written = decl
            .type_params
            .len()
            .saturating_sub(decl.assoc_types.len());
        Some(Self {
            module: module.map(str::to_string),
            written_params: decl.type_params[..written].to_vec(),
            assoc: decl
                .assoc_types
                .iter()
                .map(|assoc| (assoc.name.clone(), assoc.bounds.clone()))
                .collect(),
        })
    }
}

fn collect_namespace(
    namespace: &ModuleNamespace,
    prefix: &str,
    out: &mut BTreeMap<String, AssocTrait>,
) {
    for (name, info) in &namespace.traits {
        if let Some(found) = AssocTrait::from_decl(&info.decl, Some(&info.module_name)) {
            out.insert(format!("{prefix}.{name}"), found);
        }
    }
    for (name, child) in &namespace.modules {
        collect_namespace(child, &format!("{prefix}.{name}"), out);
    }
}

/// Fills in the associated-type arguments of every function and method
/// bound that leaves them out.
pub(super) fn expand(module: &mut Module, context: &mut ModuleContext) {
    let mut traits = BTreeMap::new();
    for (name, binding) in &context.imported_bindings {
        match binding {
            ImportedBinding::Trait(info) => {
                if let Some(found) = AssocTrait::from_decl(&info.decl, Some(&info.module_name)) {
                    traits.insert(name.clone(), found);
                }
            }
            ImportedBinding::Module(namespace) => collect_namespace(namespace, name, &mut traits),
            _ => {}
        }
    }
    for item in &module.items {
        if let Item::Trait(decl) = item {
            match AssocTrait::from_decl(decl, None) {
                Some(found) => {
                    traits.insert(decl.name.clone(), found);
                }
                None => {
                    traits.remove(&decl.name);
                }
            }
        }
    }
    if traits.is_empty() {
        return;
    }
    let mut needed = Vec::new();
    for item in &mut module.items {
        let methods = match item {
            Item::Function(function) => std::slice::from_mut(function),
            Item::Class(class) => class.methods.as_mut_slice(),
            Item::Impl(impl_decl) => impl_decl.methods.as_mut_slice(),
            Item::Trait(trait_decl) => trait_decl.methods.as_mut_slice(),
            _ => &mut [],
        };
        for method in methods {
            expand_function(method, &traits, &context.module_registry, &mut needed);
        }
    }
    // An imported trait's associated-type bound names a trait of its own
    // module, which this module may not import: bind it under its
    // qualified name.
    for (qualified, info) in needed {
        context
            .imported_bindings
            .entry(qualified)
            .or_insert(ImportedBinding::Trait(info));
    }
}

fn expand_function(
    function: &mut FunctionDecl,
    traits: &BTreeMap<String, AssocTrait>,
    registry: &BTreeMap<String, ModuleNamespace>,
    needed: &mut Vec<(String, TraitInfo)>,
) {
    let params = function.type_params.clone();
    for param in params {
        let Some(bounds) = function.type_param_bounds.get(&param).cloned() else {
            continue;
        };
        let mut rewritten = Vec::with_capacity(bounds.len());
        let mut hidden = Vec::new();
        for bound in bounds {
            // A bound that writes every argument, or names another trait,
            // stays as written.
            let Some((name, args, found)) = bound.named_parts().and_then(|(name, args)| {
                traits
                    .get(name)
                    .filter(|found| args.len() == found.written_params.len())
                    .map(|found| (name.to_string(), args.to_vec(), found))
            }) else {
                rewritten.push(bound);
                continue;
            };
            let mut substitutions = BTreeMap::new();
            for (written, arg) in found.written_params.iter().zip(&args) {
                substitutions.insert(written.clone(), arg.clone());
            }
            substitutions.insert("Self".to_string(), named(&param, bound.span));
            for (assoc, _) in &found.assoc {
                substitutions.insert(
                    crate::ast::assoc_type_param_name(assoc),
                    named(&format!("{param}.{assoc}"), bound.span),
                );
            }
            let mut args = args;
            for (assoc, assoc_bounds) in &found.assoc {
                let hidden_name = format!("{param}.{assoc}");
                args.push(named(&hidden_name, bound.span));
                let lowered = assoc_bounds
                    .iter()
                    .map(|assoc_bound| {
                        let mut lowered = substitute(assoc_bound, &substitutions, bound.span);
                        if let TypeRefKind::Named { name, .. } = &mut lowered.kind {
                            let owned = found.module.as_ref().and_then(|owner| {
                                registry
                                    .get(owner)
                                    .filter(|_| !name.contains('.'))
                                    .and_then(|namespace| namespace.all_traits.get(name.as_str()))
                                    .map(|info| (format!("{owner}.{name}"), info.clone()))
                            });
                            if let Some((qualified, info)) = owned {
                                *name = qualified.clone();
                                needed.push((qualified, info));
                            }
                        }
                        lowered
                    })
                    .collect::<Vec<_>>();
                hidden.push((hidden_name, lowered));
            }
            rewritten.push(TypeRef {
                kind: TypeRefKind::Named { name, args },
                indirect: bound.indirect,
                span: bound.span,
            });
        }
        function.type_param_bounds.insert(param.clone(), rewritten);
        for (hidden_name, hidden_bounds) in hidden {
            function
                .type_params
                .retain(|existing| *existing != hidden_name);
            function.type_params.push(hidden_name.clone());
            function
                .type_param_bounds
                .entry(hidden_name)
                .or_default()
                .extend(hidden_bounds);
        }
    }
}

fn named(name: &str, span: crate::diag::Span) -> TypeRef {
    TypeRef {
        kind: TypeRefKind::Named {
            name: name.to_string(),
            args: Vec::new(),
        },
        indirect: false,
        span,
    }
}

/// `ty` with each bare name in `substitutions` replaced, at `span`.
fn substitute(
    ty: &TypeRef,
    substitutions: &BTreeMap<String, TypeRef>,
    span: crate::diag::Span,
) -> TypeRef {
    let kind = match &ty.kind {
        TypeRefKind::Named { name, args } if args.is_empty() => {
            if let Some(replacement) = substitutions.get(name) {
                return replacement.clone();
            }
            ty.kind.clone()
        }
        TypeRefKind::Named { name, args } => TypeRefKind::Named {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| substitute(arg, substitutions, span))
                .collect(),
        },
        TypeRefKind::Tuple(items) => TypeRefKind::Tuple(
            items
                .iter()
                .map(|item| substitute(item, substitutions, span))
                .collect(),
        ),
        other => other.clone(),
    };
    TypeRef {
        kind,
        indirect: ty.indirect,
        span,
    }
}
