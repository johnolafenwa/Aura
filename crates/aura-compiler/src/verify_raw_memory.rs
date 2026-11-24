//! A raw store may change an Aura place through an address the proof does
//! not track. Propagate this limitation to callers before collecting sites
//! for check removal, including when the user filters verification by name.
use std::collections::{BTreeMap, BTreeSet};

use crate::mir::{CallTarget, Instruction, MethodDispatch, MirFunction, MirModule, Rvalue};
use crate::sema::Type;

fn uses_typed_pointer(function: &MirFunction) -> bool {
    let exposes_storage = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            let Instruction::Assign { value, .. } = instruction else {
                return false;
            };
            match value {
                Rvalue::CFieldAddress { .. }
                | Rvalue::ModuleAddress { .. }
                | Rvalue::CPointerAccess { .. } => true,
                Rvalue::Call {
                    callee: CallTarget::Name(name),
                    ..
                } => name == crate::mir::fixed_arrays::ADDRESS,
                _ => false,
            }
        });
    exposes_storage
        || function
            .local_types
            .iter()
            .map(|local| &local.ty)
            .chain(function.params.iter().map(|param| &param.ty))
            .chain(std::iter::once(&function.return_type))
            .any(|ty| matches!(ty, Type::Named(name, _) if name == "ptr"))
}

fn writes_memory(name: &str) -> bool {
    let Some(op) = name.strip_prefix("mem::") else {
        return false;
    };
    op.starts_with("write_")
        || op.starts_with("volatile_write_")
        || op.starts_with("atomic_store_")
        || op.starts_with("atomic_compare_exchange_")
        || op.starts_with("atomic_add_")
        || op.starts_with("atomic_swap_")
}

/// For a program that can form raw pointers, the verifier refuses direct
/// writes, calls into foreign code and calls whose target is selected at
/// run time. Static calls propagate only their own callee's effects.
pub(super) fn affected_functions(module: &MirModule) -> BTreeSet<&str> {
    if !module
        .functions
        .iter()
        .any(|f| !crate::mir::is_builtin_wrapper(f) && uses_typed_pointer(f))
    {
        return BTreeSet::new();
    }
    let mut callers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut affected = BTreeSet::new();
    for function in &module.functions {
        if crate::mir::is_builtin_wrapper(function) {
            continue;
        }
        let name = function.name.as_str();
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let value = match instruction {
                Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => value,
                Instruction::PushCleanup { .. } => {
                    affected.insert(name);
                    continue;
                }
                _ => continue,
            };
            let callee = match value {
                Rvalue::Call { callee, .. } => callee,
                Rvalue::CPointerAccess { store: Some(_), .. }
                | Rvalue::CFieldAddress { .. }
                | Rvalue::ModuleAddress { .. }
                | Rvalue::StartTask { .. }
                | Rvalue::Closure { .. } => {
                    affected.insert(name);
                    continue;
                }
                Rvalue::Try {
                    from: Some(target), ..
                }
                | Rvalue::ModuleConstant {
                    initializer: target,
                    ..
                } => {
                    callers.entry(target.as_str()).or_default().push(name);
                    continue;
                }
                _ => continue,
            };
            let target = match callee {
                CallTarget::Name(target) => target.as_str(),
                CallTarget::Runtime { op, .. } => op.name(),
                CallTarget::Member {
                    dispatch: Some(MethodDispatch::Static { function }),
                    ..
                }
                | CallTarget::TraitMember {
                    dispatch: Some(MethodDispatch::Static { function }),
                    ..
                } => function,
                _ => {
                    affected.insert(name);
                    continue;
                }
            };
            if writes_memory(target) || target == crate::mir::fixed_arrays::ADDRESS {
                affected.insert(name);
            }
            callers.entry(target).or_default().push(name);
        }
    }
    let mut pending: Vec<_> = affected.iter().copied().collect();
    while let Some(name) = pending.pop() {
        if let Some(parents) = callers.remove(name) {
            for parent in parents {
                if affected.insert(parent) {
                    pending.push(parent);
                }
            }
        }
    }
    affected
}
