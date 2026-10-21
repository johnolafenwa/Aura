//! Drops builtin module wrappers that no program function reaches before
//! native code generation.
//!
//! Lowering emits a MIR wrapper for every builtin module function, such as
//! `toml::is_valid`, whether or not the program imports it. Most programs
//! call a few, so generating the rest only costs compile time and object
//! size. Every function from the program itself is kept, so codegen still
//! validates and diagnoses code the program never calls. The walk is
//! conservative: any string inside a kept function that names a wrapper keeps
//! that wrapper, which covers function values and parameter defaults.
//! Builtin module constants, such as `math.e`, keep their initializers.

use std::collections::{HashMap, HashSet};

use crate::mir::{CallTarget, Instruction, MirFunction, MirModule, Rvalue};

pub(super) fn prune_unreachable_functions(module: &MirModule) -> MirModule {
    let wrappers = module
        .functions
        .iter()
        .filter(|function| is_builtin_wrapper(function))
        .map(|function| (function.name.as_str(), function))
        .collect::<HashMap<_, _>>();
    if wrappers.is_empty() {
        return module.clone();
    }

    // Module constants, class methods, and trait methods name their functions
    // outside any function body, so they are roots alongside the program.
    let named_roots = module
        .constants
        .iter()
        .map(|constant| constant.initializer.as_str())
        .chain(
            module
                .classes
                .iter()
                .flat_map(|class| class.methods.iter())
                .chain(
                    module
                        .trait_impls
                        .iter()
                        .flat_map(|implementation| implementation.methods.iter()),
                )
                .map(|method| method.function_name.as_str()),
        );
    let mut kept = HashSet::new();
    let mut pending = module
        .functions
        .iter()
        .filter(|function| !is_builtin_wrapper(function))
        .chain(module.top_level.iter())
        .collect::<Vec<_>>();
    for name in named_roots {
        if let Some(wrapper) = wrappers.get(name) {
            if kept.insert(name.to_string()) {
                pending.push(wrapper);
            }
        }
    }
    let mut names = Vec::new();
    while let Some(function) = pending.pop() {
        // MIR is plain serializable data. If a function ever fails to
        // serialize, keep every wrapper rather than guess what it names.
        let Ok(value) = serde_json::to_value(function) else {
            return module.clone();
        };
        collect_strings(&value, &mut names);
        collect_named_op_callees(function, &mut names);
        for name in names.drain(..) {
            if let Some(wrapper) = wrappers.get(name.as_str()) {
                if kept.insert(name) {
                    pending.push(wrapper);
                }
            }
        }
    }

    let mut pruned = module.clone();
    pruned
        .functions
        .retain(|function| !is_builtin_wrapper(function) || kept.contains(&function.name));
    pruned
}

fn is_builtin_wrapper(function: &MirFunction) -> bool {
    // A `Display` wrapper is a root: the renderer reaches it by its name's
    // prefix, not through a call.
    crate::mir::display_wrapper_type_name(&function.name).is_none()
        && crate::mir::is_builtin_wrapper(function)
}

/// The callee name of each named runtime op in `function`, such as
/// `fs::exists`. A resolved op serializes as its variant, not as the name
/// its wrapper has.
fn collect_named_op_callees(function: &MirFunction, out: &mut Vec<String>) {
    let values = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => Some(value),
            _ => None,
        });
    for value in values {
        if let Rvalue::Call {
            callee: CallTarget::Runtime {
                op, receiver: None, ..
            },
            ..
        } = value
        {
            out.push(op.name().to_string());
        }
    }
}

fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, item) in fields {
                out.push(key.clone());
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}
