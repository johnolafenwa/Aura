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
use crate::ast::{TraitDecl, TypeRef, TypeRefKind};

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
    inherit_assoc_types(module, &mut traits, &context.module_registry, &mut needed);
    for item in &mut module.items {
        let methods = match item {
            Item::Function(function) => std::slice::from_mut(function),
            Item::Class(class) => {
                // A class keeps its written arity: each method gains the
                // hidden `S.Iter` of a class bound, and a construction
                // infers it to check the bound.
                let hidden = expand_bounds(
                    &mut class.type_params,
                    &mut class.type_param_bounds,
                    &traits,
                    &context.module_registry,
                    &mut needed,
                );
                let written = class.type_params.len() - hidden.len();
                class.type_params.truncate(written);
                // Each method also restates the class bound that fixes its
                // hidden parameters, so a call infers them from the receiver.
                let owners = class
                    .type_param_bounds
                    .iter()
                    .filter(|(param, _)| {
                        let prefix = format!("{param}.");
                        hidden.iter().any(|(name, _)| name.starts_with(&prefix))
                    })
                    .map(|(param, bounds)| (param.clone(), bounds.clone()))
                    .collect::<Vec<_>>();
                for method in &mut class.methods {
                    for (param, bounds) in &owners {
                        method
                            .type_param_bounds
                            .entry(param.clone())
                            .or_default()
                            .extend(bounds.iter().cloned());
                    }
                    for (hidden_name, hidden_bounds) in &hidden {
                        method
                            .type_params
                            .retain(|existing| existing != hidden_name);
                        method.type_params.push(hidden_name.clone());
                        method
                            .type_param_bounds
                            .entry(hidden_name.clone())
                            .or_default()
                            .extend(hidden_bounds.iter().cloned());
                    }
                }
                for hidden_name in hidden.iter().map(|(name, _)| name) {
                    class.type_param_bounds.remove(hidden_name);
                }
                class.methods.as_mut_slice()
            }
            Item::Impl(impl_decl) => {
                // An impl's hidden `S.Iter` is one more impl type parameter,
                // inferred from `S` whenever the impl is matched.
                expand_bounds(
                    &mut impl_decl.type_params,
                    &mut impl_decl.type_param_bounds,
                    &traits,
                    &context.module_registry,
                    &mut needed,
                );
                impl_decl.methods.as_mut_slice()
            }
            Item::Enum(enum_decl) => {
                // An enum keeps its written arity, as a class does: a
                // variant construction infers the hidden `S.Iter` to check
                // the bound.
                let hidden = expand_bounds(
                    &mut enum_decl.type_params,
                    &mut enum_decl.type_param_bounds,
                    &traits,
                    &context.module_registry,
                    &mut needed,
                );
                let written = enum_decl.type_params.len() - hidden.len();
                enum_decl.type_params.truncate(written);
                for hidden_name in hidden.iter().map(|(name, _)| name) {
                    enum_decl.type_param_bounds.remove(hidden_name);
                }
                &mut []
            }
            Item::Trait(trait_decl) => trait_decl.methods.as_mut_slice(),
            _ => &mut [],
        };
        for method in methods {
            expand_bounds(
                &mut method.type_params,
                &mut method.type_param_bounds,
                &traits,
                &context.module_registry,
                &mut needed,
            );
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

/// Gives each trait the associated types of its supertraits. A supertrait
/// written with its own arguments only, as in `trait Bag[T]: IntoIterator[T]`,
/// becomes `IntoIterator[T, Self.Iter]`, and `Bag` gains `Iter` as an
/// associated type of its own with the bounds `IntoIterator` gives it. A
/// bound `B: Bag[T]` then names `B.Iter` like any other, and an
/// implementation of `Bag` takes `Iter` from the type's implementation of
/// `IntoIterator`.
fn inherit_assoc_types(
    module: &mut Module,
    traits: &mut BTreeMap<String, AssocTrait>,
    registry: &BTreeMap<String, ModuleNamespace>,
    needed: &mut Vec<(String, TraitInfo)>,
) {
    // A supertrait declared later in the module may inherit in turn, so
    // repeat until no trait changes.
    loop {
        let mut changed = false;
        for item in &mut module.items {
            let Item::Trait(decl) = item else {
                continue;
            };
            for index in 0..decl.supertraits.len() {
                let supertrait = decl.supertraits[index].clone();
                let Some((name, args, found)) =
                    supertrait.named_parts().and_then(|(name, args)| {
                        traits
                            .get(name)
                            .filter(|found| args.len() == found.written_params.len())
                            .map(|found| (name.to_string(), args.to_vec(), found.clone()))
                    })
                else {
                    continue;
                };
                let mut substitutions = BTreeMap::new();
                for (written, arg) in found.written_params.iter().zip(&args) {
                    substitutions.insert(written.clone(), arg.clone());
                }
                let mut args = args;
                for (assoc, assoc_bounds) in &found.assoc {
                    let hidden_name = crate::ast::assoc_type_param_name(assoc);
                    args.push(named(&hidden_name, supertrait.span));
                    if decl.assoc_types.iter().any(|own| own.name == *assoc) {
                        continue;
                    }
                    let bounds = assoc_bounds
                        .iter()
                        .map(|assoc_bound| {
                            let mut lowered =
                                substitute(assoc_bound, &substitutions, supertrait.span);
                            qualify(&mut lowered, found.module.as_deref(), registry, needed);
                            lowered
                        })
                        .collect();
                    decl.type_params.push(hidden_name);
                    decl.assoc_types.push(crate::ast::AssocTypeDecl {
                        name: assoc.clone(),
                        bounds,
                        span: supertrait.span,
                    });
                }
                // An imported supertrait is named by its module, so a module
                // that imports only this trait still resolves it.
                let mut expanded = TypeRef {
                    kind: TypeRefKind::Named { name, args },
                    indirect: supertrait.indirect,
                    span: supertrait.span,
                };
                if found.module.is_some() {
                    qualify(&mut expanded, found.module.as_deref(), registry, needed);
                }
                decl.supertraits[index] = expanded;
                changed = true;
            }
            if changed {
                if let Some(found) = AssocTrait::from_decl(decl, None) {
                    traits.insert(decl.name.clone(), found);
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// The associated types of `decl` that a supertrait fixes: each is passed
/// to a supertrait as one of its own associated types, as `Self.Iter` in
/// `IntoIterator[T, Self.Iter]`, so the type's implementation of that
/// supertrait decides it.
pub(super) fn inherited_assoc_names(
    decl: &TraitDecl,
    traits: &BTreeMap<String, TraitInfo>,
) -> Vec<String> {
    let mut names = Vec::new();
    for supertrait in &decl.supertraits {
        let Some((name, args)) = supertrait.named_parts() else {
            continue;
        };
        let Some(info) = traits.get(name) else {
            continue;
        };
        let written = args.len().saturating_sub(info.decl.assoc_types.len());
        for arg in &args[written..] {
            let TypeRefKind::Named { name, args } = &arg.kind else {
                continue;
            };
            let Some(assoc) = name.strip_prefix("Self.") else {
                continue;
            };
            if args.is_empty()
                && decl.assoc_types.iter().any(|own| own.name == assoc)
                && !names.iter().any(|known| known == assoc)
            {
                names.push(assoc.to_string());
            }
        }
    }
    names
}

/// The placeholder an implementation's trait arguments carry for an
/// associated type it leaves to a supertrait's implementation.
pub(super) fn inherited_placeholder(name: &str, span: crate::diag::Span) -> TypeRef {
    named(&crate::ast::assoc_type_param_name(name), span)
}

/// Rewrites the bounds of `type_params` that leave out a trait's
/// associated types, appends each hidden `S.Name` parameter with its
/// bounds, and returns the hidden parameters it added.
fn expand_bounds(
    type_params: &mut Vec<String>,
    type_param_bounds: &mut BTreeMap<String, Vec<TypeRef>>,
    traits: &BTreeMap<String, AssocTrait>,
    registry: &BTreeMap<String, ModuleNamespace>,
    needed: &mut Vec<(String, TraitInfo)>,
) -> Vec<(String, Vec<TypeRef>)> {
    let mut added = Vec::new();
    let params = type_params.clone();
    for param in params {
        let Some(bounds) = type_param_bounds.get(&param).cloned() else {
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
                        qualify(&mut lowered, found.module.as_deref(), registry, needed);
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
        type_param_bounds.insert(param.clone(), rewritten);
        for (hidden_name, hidden_bounds) in hidden {
            type_params.retain(|existing| *existing != hidden_name);
            type_params.push(hidden_name.clone());
            type_param_bounds
                .entry(hidden_name.clone())
                .or_default()
                .extend(hidden_bounds.iter().cloned());
            added.push((hidden_name, hidden_bounds));
        }
    }
    added
}

/// Spells the trait an associated-type bound names so that a module
/// importing only the bounded trait resolves it: a bare name is a trait of
/// the declaring module `owner`, and a dotted one names its module already.
fn qualify(
    bound: &mut TypeRef,
    owner: Option<&str>,
    registry: &BTreeMap<String, ModuleNamespace>,
    needed: &mut Vec<(String, TraitInfo)>,
) {
    let TypeRefKind::Named { name, .. } = &mut bound.kind else {
        return;
    };
    let (module, base) = match name.rsplit_once('.') {
        Some((module, base)) => (Some(module.to_string()), base.to_string()),
        None => (owner.map(str::to_string), name.clone()),
    };
    let Some(module) = module else {
        return;
    };
    let Some(info) = registry
        .get(&module)
        .and_then(|namespace| namespace.all_traits.get(base.as_str()))
    else {
        return;
    };
    let qualified = format!("{module}.{base}");
    *name = qualified.clone();
    needed.push((qualified, info.clone()));
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

/// The hidden `S.Name` parameters a class bound names after expansion: a
/// class keeps only its written parameters, so its bounds are lowered with
/// these in scope as well.
pub(super) fn hidden_bound_params(
    type_params: &[String],
    bounds: &BTreeMap<String, Vec<TypeRef>>,
) -> Vec<String> {
    let mut hidden = Vec::new();
    for (param, param_bounds) in bounds {
        let prefix = format!("{param}.");
        for bound in param_bounds {
            let Some((_, args)) = bound.named_parts() else {
                continue;
            };
            for arg in args {
                let TypeRefKind::Named { name, args } = &arg.kind else {
                    continue;
                };
                if args.is_empty()
                    && name.starts_with(&prefix)
                    && type_params.contains(param)
                    && !hidden.contains(name)
                {
                    hidden.push(name.clone());
                }
            }
        }
    }
    hidden
}
