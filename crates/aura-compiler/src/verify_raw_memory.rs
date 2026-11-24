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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mir::{CleanupAction, Operand};

    #[test]
    fn proof_boundaries_follow_implicit_and_dispatched_calls() {
        let source = "def effect(p: ptr[uint8]) -> None:\n    p.write(3)\n\ndef caller() -> None:\n    return\n\ndef parent() -> None:\n    caller()\n\ndef pure() -> None:\n    return\n";
        let program =
            crate::check_module_with_builtin_imports(crate::parse_source(source).unwrap()).unwrap();
        let mut module = crate::mir::lower_for_verification(&program);
        let caller = module
            .functions
            .iter()
            .position(|f| f.name == "caller")
            .unwrap();
        let assign = |value| Instruction::Assign {
            target: "result".into(),
            value,
        };
        let call = |callee| {
            assign(Rvalue::Call {
                callee,
                args: vec![],
                type_args: Default::default(),
            })
        };
        let dispatch = Some(MethodDispatch::Static {
            function: "effect".into(),
        });
        for operation in [
            assign(Rvalue::Try {
                value: Operand::Unit,
                releases_on_error: vec![],
                from: Some("effect".into()),
            }),
            assign(Rvalue::ModuleConstant {
                key: "data".into(),
                initializer: "effect".into(),
            }),
            call(CallTarget::Member {
                object: Operand::Unit,
                field: "write".into(),
                receiver_place: None,
                dispatch: dispatch.clone(),
            }),
            call(CallTarget::TraitMember {
                object: Operand::Unit,
                field: "write".into(),
                trait_name: "Writer".into(),
                trait_args: vec![],
                receiver_place: None,
                dispatch,
            }),
            call(CallTarget::Value(Operand::Place("callback".into()))),
            Instruction::PushCleanup {
                place: "resource".into(),
                action: CleanupAction::ResourceClose,
                callee: None,
            },
        ] {
            let body = &mut module.functions[caller].blocks[0].instructions;
            body.push(operation);
            let affected = affected_functions(&module);
            assert!(
                ["effect", "caller", "parent"]
                    .iter()
                    .all(|name| affected.contains(name)),
                "{affected:?}"
            );
            assert!(!affected.contains("pure"));
            module.functions[caller].blocks[0].instructions.pop();
        }

        for (operation, writes) in [
            ("write_int16", true),
            ("volatile_write_float32", true),
            ("atomic_store_uint16", true),
            ("atomic_compare_exchange_uint8", true),
            ("atomic_add_uint32", true),
            ("atomic_swap_uint64", true),
            ("volatile_read_int32", false),
            ("atomic_load_uint8", false),
        ] {
            assert_eq!(writes_memory(&format!("mem::{operation}")), writes);
        }
    }
}
