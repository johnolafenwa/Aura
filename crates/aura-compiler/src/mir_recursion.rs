//! The functions of a module that a call cycle can reenter (Batch 8 step
//! 8d).
//!
//! A loop yields to the scheduler at its backedges. Recursion with no loop
//! has no backedge, so the entries of these functions count scheduling fuel
//! instead, and deep recursion yields like a loop does.
//!
//! The call graph is conservative. A function calls every function whose
//! name appears anywhere in its body, which covers direct calls, method
//! dispatch, closures, function values, and cleanup callees. A function
//! with a call the graph cannot resolve, such as a call through a function
//! value, a dynamic member, a builtin, or a runtime op that may call back
//! into Aura code, calls every function that escapes: one named somewhere
//! other than as a direct callee, a method, or a `Display` wrapper. A
//! function on a cycle of that graph is recursive. Missing a cycle only
//! leaves recursion that never yields, and an extra function only pays for
//! a counter at its entry.

use std::collections::{HashMap, HashSet};

use super::{CallTarget, Instruction, MethodDispatch, MirFunction, MirModule, Rvalue};

/// The names of the functions of `module` that lie on a call cycle,
/// including a function that calls itself.
pub fn recursive_functions(module: &MirModule) -> HashSet<String> {
    let functions = module
        .functions
        .iter()
        .chain(module.top_level.iter())
        .collect::<Vec<_>>();
    let index = functions
        .iter()
        .enumerate()
        .map(|(position, function)| (function.name.as_str(), position))
        .collect::<HashMap<_, _>>();

    let mut escaping = vec![false; functions.len()];
    for function in &module.functions {
        if super::display_wrapper_type_name(&function.name).is_some() {
            escaping[index[function.name.as_str()]] = true;
        }
    }
    for method in module
        .classes
        .iter()
        .flat_map(|class| class.methods.iter())
        .chain(
            module
                .trait_impls
                .iter()
                .flat_map(|implementation| implementation.methods.iter()),
        )
    {
        if let Some(&position) = index.get(method.function_name.as_str()) {
            escaping[position] = true;
        }
    }

    let mut edges = vec![Vec::new(); functions.len()];
    let mut opaque = vec![false; functions.len()];
    for (position, function) in functions.iter().enumerate() {
        let mut named = HashMap::<usize, usize>::new();
        for name in body_strings(function) {
            if let Some(&callee) = index.get(name.as_str()) {
                *named.entry(callee).or_default() += 1;
            }
        }
        let mut direct = HashMap::<usize, usize>::new();
        for value in call_values(function) {
            let Rvalue::Call { callee, .. } = value else {
                if matches!(value, Rvalue::FormatString { .. }) {
                    opaque[position] = true;
                }
                continue;
            };
            match callee {
                CallTarget::Name(name) => match index.get(name.as_str()) {
                    Some(&target) => *direct.entry(target).or_default() += 1,
                    None => opaque[position] = true,
                },
                CallTarget::Extern(_) => {}
                CallTarget::Member { dispatch, .. } | CallTarget::TraitMember { dispatch, .. } => {
                    if !dispatch.as_ref().is_some_and(is_static_dispatch) {
                        opaque[position] = true;
                    }
                }
                CallTarget::Value(_) | CallTarget::Runtime { .. } => opaque[position] = true,
            }
        }
        for (&callee, &count) in &named {
            if count > direct.get(&callee).copied().unwrap_or(0) {
                escaping[callee] = true;
            }
        }
        let mut callees = named.into_keys().collect::<Vec<_>>();
        callees.sort_unstable();
        edges[position] = callees;
    }

    let escaping = (0..functions.len())
        .filter(|&position| escaping[position])
        .collect::<Vec<_>>();
    for (position, callees) in edges.iter_mut().enumerate() {
        if opaque[position] {
            callees.extend(escaping.iter().copied());
            callees.sort_unstable();
            callees.dedup();
        }
    }

    cyclic_nodes(&edges)
        .into_iter()
        .map(|position| functions[position].name.clone())
        .collect()
}

fn is_static_dispatch(dispatch: &MethodDispatch) -> bool {
    match dispatch {
        MethodDispatch::Static { .. } | MethodDispatch::Nothing => true,
        MethodDispatch::Union(arms) => arms.iter().all(is_static_dispatch),
        _ => false,
    }
}

/// Every string in `function` other than its own name. MIR is plain
/// serializable data; if a function ever fails to serialize, its body
/// names no function, and its calls stay opaque.
fn body_strings(function: &MirFunction) -> Vec<String> {
    let mut strings = Vec::new();
    if let Ok(serde_json::Value::Object(mut fields)) = serde_json::to_value(function) {
        fields.remove("name");
        for value in fields.values() {
            collect_strings(value, &mut strings);
        }
    }
    strings
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
            for item in fields.values() {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

fn call_values(function: &MirFunction) -> impl Iterator<Item = &Rvalue> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => Some(value),
            _ => None,
        })
}

/// The nodes of `edges` that lie on a cycle: those in a strongly connected
/// component of more than one node, or with an edge to themselves. An
/// iterative Tarjan walk, so a long call chain cannot overflow the stack.
fn cyclic_nodes(edges: &[Vec<usize>]) -> Vec<usize> {
    const UNVISITED: usize = usize::MAX;
    let count = edges.len();
    let mut order = vec![UNVISITED; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut next_order = 0;
    let mut cyclic = Vec::new();

    for root in 0..count {
        if order[root] != UNVISITED {
            continue;
        }
        // Each frame is a node and the position of its next edge.
        let mut frames = vec![(root, 0usize)];
        order[root] = next_order;
        low[root] = next_order;
        next_order += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(node, edge)) = frames.last() {
            if let Some(&next) = edges[node].get(edge) {
                let top = frames.len() - 1;
                frames[top].1 += 1;
                if order[next] == UNVISITED {
                    order[next] = next_order;
                    low[next] = next_order;
                    next_order += 1;
                    stack.push(next);
                    on_stack[next] = true;
                    frames.push((next, 0));
                } else if on_stack[next] {
                    low[node] = low[node].min(order[next]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] != order[node] {
                continue;
            }
            let mut component = Vec::new();
            while let Some(member) = stack.pop() {
                on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            if component.len() > 1 || edges[node].contains(&node) {
                cyclic.extend(component);
            }
        }
    }
    cyclic.sort_unstable();
    cyclic
}

#[cfg(test)]
#[path = "mir_recursion_tests.rs"]
mod tests;
