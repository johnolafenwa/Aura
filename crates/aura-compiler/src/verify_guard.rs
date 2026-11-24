//! Which trait implementation methods may assume their class invariant.
//!
//! A method of an `impl Trait for Class:` block cannot be `public`, so no
//! run checks the class invariant around it. Its proof may still assume
//! the invariant on entry when every way of reaching the method is proved
//! to establish it:
//!
//! - a call that names the method proves the invariant of its receiver;
//! - a call on a trait object relies on the conversion that made the
//!   object, which proves the invariant of the value it takes, and on each
//!   `mut self` implementation method, which proves it again on return.
//!
//! Every other way of reaching the method is found here, by reading the
//! MIR of the whole program, and the method then assumes nothing: a call
//! on a union, a call from the `Display` renderer, a name used as a value
//! or a cleanup, and a call on a value whose type is a type parameter.
//! Generic functions are verified as copies for the type arguments their
//! calls use, where the call names the method. Their originals run only
//! where a function without type parameters calls one directly, so an
//! original's calls count against that caller.
//!
//! Modules are verified one at a time, and each sees only what it imports.
//! A reference in the method's own module, or anywhere when the method's
//! module is the one being verified, withdraws the assumption: every
//! module that sees the method sees that reference too. A reference from
//! the module being verified to a method of an imported module cannot
//! withdraw it, since the imported module was verified without seeing it.
//! The function that holds such a reference gets an obligation instead.

use std::collections::{BTreeMap, BTreeSet};

use crate::mir::{CallTarget, Instruction, MethodDispatch, MirFunction, Rvalue};

/// What the verifier knows about trait implementation methods of classes
/// with invariants.
#[derive(Default)]
pub(crate) struct Guards {
    /// The methods that assume their class invariant on entry.
    pub(crate) guarded: BTreeSet<String>,
    /// For a function of the verified module that reaches a guarded method
    /// of another module in a way that proves nothing, that method.
    pub(crate) charged: BTreeMap<String, String>,
}

/// The trait and class of a trait implementation method, read from its
/// MIR name `Trait for Class.method`. Generic classes and copies of
/// generic methods are left out.
pub(crate) fn impl_method_parts(name: &str) -> Option<(&str, &str)> {
    let (trait_name, rest) = name.split_once(" for ")?;
    let (class, method) = rest.rsplit_once('.')?;
    (!class.contains('[') && !method.contains('[')).then_some((trait_name, class))
}

/// Whether `function` names a type parameter, or a type lowering left
/// unknown: its member calls may be decided when the program runs.
fn is_generic(function: &MirFunction) -> bool {
    let types = format!(
        "{:?}{:?}{:?}",
        function.params, function.local_types, function.return_type
    );
    types.contains("TypeParam(") || types.contains("\"Unknown\"")
}

/// How often each of `wanted` appears as a string anywhere in `value`.
fn count_names(
    value: &serde_json::Value,
    wanted: &BTreeSet<&str>,
    found: &mut BTreeMap<String, usize>,
) {
    match value {
        serde_json::Value::String(text) if wanted.contains(text.as_str()) => {
            *found.entry(text.clone()).or_default() += 1;
        }
        serde_json::Value::Array(items) => {
            for item in items {
                count_names(item, wanted, found);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values() {
                count_names(item, wanted, found);
            }
        }
        _ => {}
    }
}

/// The candidates `function` reaches in a way that proves no invariant,
/// and the generic functions it names.
fn references(
    function: &MirFunction,
    candidates: &BTreeSet<&str>,
    generic: &BTreeSet<&str>,
    top_level: Option<&str>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let wanted: BTreeSet<&str> = candidates.union(generic).copied().collect();
    let mut named = BTreeMap::new();
    // MIR is plain serializable data, so every name a body holds is one of
    // its strings, wherever a later MIR change may put it.
    let blocks = serde_json::to_value(&function.blocks).unwrap_or_default();
    count_names(&blocks, &wanted, &mut named);
    let mut proved: BTreeMap<&str, usize> = BTreeMap::new();
    let mut converts_type_param = false;
    let dynamic = generic.contains(function.name.as_str());
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        let (Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. }) =
            instruction
        else {
            continue;
        };
        match value {
            Rvalue::Call {
                callee:
                    CallTarget::Member {
                        dispatch: Some(MethodDispatch::Static { function: callee }),
                        ..
                    }
                    | CallTarget::TraitMember {
                        dispatch: Some(MethodDispatch::Static { function: callee }),
                        ..
                    },
                ..
            } => *proved.entry(callee.as_str()).or_default() += 1,
            // A trait object holds its invariant, so a call on one needs
            // no proof. In a generic function the receiver may be a value
            // of a type parameter instead.
            Rvalue::Call {
                callee:
                    CallTarget::TraitMember {
                        dispatch: Some(MethodDispatch::Candidates { candidates, .. }),
                        ..
                    },
                ..
            } if !dynamic => {
                for candidate in candidates {
                    *proved.entry(candidate.function.as_str()).or_default() += 1;
                }
            }
            Rvalue::TraitObject { held, .. } => {
                converts_type_param |= format!("{held:?}").contains("TypeParam(");
            }
            _ => {}
        }
    }
    // The renderer calls a `Display` wrapper on any value it prints, and
    // module-level code is not verified, so neither proves anything.
    let rendered = crate::mir::display_wrapper_type_name(&function.name).is_some()
        || top_level == Some(function.name.as_str());
    let mut unproved = BTreeSet::new();
    let mut generics = BTreeSet::new();
    for (name, count) in named {
        if generic.contains(name.as_str()) {
            generics.insert(name.clone());
        }
        let proved = proved.get(name.as_str()).copied().unwrap_or(0);
        if candidates.contains(name.as_str()) && (rendered || count > proved) {
            unproved.insert(name);
        }
    }
    // A value of a type parameter that becomes a trait object may be of
    // any class.
    if converts_type_param {
        unproved.extend(candidates.iter().map(|name| name.to_string()));
    }
    (unproved, generics)
}

/// Decides which of `candidates` keep their assumption. `entry` is the
/// module being verified, and `top_level` names its module-level code.
pub(crate) fn guards(
    functions: &BTreeMap<&str, &MirFunction>,
    candidates: &BTreeSet<&str>,
    entry: &str,
    top_level: Option<&str>,
) -> Guards {
    let generic: BTreeSet<&str> = functions
        .values()
        .filter(|function| is_generic(function))
        .map(|function| function.name.as_str())
        .collect();
    let references: BTreeMap<&str, (BTreeSet<String>, BTreeSet<String>)> = functions
        .iter()
        .map(|(name, function)| (*name, references(function, candidates, &generic, top_level)))
        .collect();
    // What each function without type parameters reaches, itself or
    // through the generic originals it calls.
    let mut reached: Vec<(&MirFunction, BTreeSet<String>)> = Vec::new();
    for (name, function) in functions {
        if generic.contains(name) {
            continue;
        }
        let mut unproved = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut work = vec![*name];
        while let Some(next) = work.pop() {
            let (direct, generics) = &references[next];
            unproved.extend(direct.iter().cloned());
            for original in generics {
                if seen.insert(original.as_str()) {
                    work.push(original);
                }
            }
        }
        reached.push((function, unproved));
    }
    let module_of = |name: &str| functions[name].module_name.as_str();
    let mut guarded: BTreeSet<String> = candidates.iter().map(|name| name.to_string()).collect();
    for (function, unproved) in &reached {
        for target in unproved {
            let home = module_of(target);
            if home == entry || home == function.module_name {
                guarded.remove(target);
            }
        }
    }
    let mut charged = BTreeMap::new();
    for (function, unproved) in &reached {
        if function.module_name != entry {
            continue;
        }
        if let Some(target) = unproved.iter().find(|target| guarded.contains(*target)) {
            charged.insert(function.name.clone(), target.clone());
        }
    }
    Guards { guarded, charged }
}
