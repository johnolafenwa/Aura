//! Finds the loops over a dict's keys that look each key up again, which
//! native code walks by entry instead.
//!
//! `for key in d.keys(): match d.lookup(key)` lowers to a copy of the keys,
//! a copy of each key read out of it, and a hashed lookup of that key: a
//! `contains_key` test fused with the entry loan that follows it. When
//! nothing changes the dict while the function runs, the copy of the keys
//! holds the dict's own keys in its order, so the loop's `i`th key is the
//! key of the dict's `i`th entry, and its lookup finds that entry. Native
//! code then makes neither copy and hashes nothing: it keeps the slot of
//! the entry it is on (see `aura_direct_map_next_slot`), and the fused test
//! lends the value in that slot (see `aura_direct_map_shadow_at`).
//!
//! A loop qualifies when the keys copy is only measured, indexed by the
//! loop, and released; the key is only tested, borrowed by the test's entry
//! loan, and released; and every instruction that names the dict only reads
//! it.

use std::collections::{HashMap, HashSet};

use crate::mir::ownership::{instruction_accesses, Access};
use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MirFunction,
    Operand, Rvalue,
};
use crate::runtime_ops::{DictOp, ListOp, RuntimeOp};

use super::element_loans::FusedEntryProbe;

/// One loop over a dict's keys that native code walks by entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct KeyLoop {
    /// The dict the keys come from.
    pub(super) dict: String,
    /// The local each key is read into.
    pub(super) key: String,
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// The runtime op of a call on a root local, with that local.
fn runtime_call(value: &Rvalue) -> Option<(RuntimeOp, &str, &[crate::mir::MirArg])> {
    match value {
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op,
                    receiver: Some(Operand::Place(receiver)),
                    ..
                },
            args,
            ..
        } if !receiver.contains('.') => Some((*op, receiver.as_str(), args.as_slice())),
        _ => None,
    }
}

/// The roots `instruction` names, as targets, operands, or places it reads.
fn mentioned_roots(instruction: &Instruction) -> HashSet<&str> {
    let mut mentions = Vec::new();
    instruction_place_mentions(instruction, &mut mentions);
    let mut roots = mentions.into_iter().map(root).collect::<HashSet<_>>();
    roots.extend(
        instruction_accesses(instruction)
            .into_iter()
            .map(|(place, _)| root(place)),
    );
    roots
}

/// The loops of `function` native code walks by entry, by the local that
/// holds the keys copy. `fused` are the function's fused `contains_key`
/// tests, and a test qualifies when its loan is in `shadow` (see
/// `element_loans::shadow_entry_copy_loans`), so the loan only lends the
/// value to borrowers. `followable` says which dicts a fused test reads
/// where they lie.
pub(super) fn entry_key_loops(
    function: &MirFunction,
    fused: &HashMap<String, FusedEntryProbe>,
    shadow: &HashSet<String>,
    followable: impl Fn(&str) -> bool,
) -> HashMap<String, KeyLoop> {
    let instructions = || function.blocks.iter().flat_map(|block| &block.instructions);
    // Each keys copy of a local dict, with the dict and the key it is read
    // into, once one is found.
    let mut loops = HashMap::<&str, (&str, Option<&str>)>::new();
    let mut rejected = HashSet::<&str>::new();
    for instruction in instructions() {
        let Instruction::Assign { target, value } = instruction else {
            continue;
        };
        if let Some((RuntimeOp::Dict(DictOp::Keys), dict, _)) = runtime_call(value) {
            if !target.contains('.') && target != dict && followable(dict) {
                if loops.insert(target, (dict, None)).is_some() {
                    rejected.insert(target);
                }
            }
        }
    }
    for instruction in instructions() {
        let Instruction::Assign { target, value } = instruction else {
            continue;
        };
        let Some((RuntimeOp::List(ListOp::IndexUnchecked), list, _)) = runtime_call(value) else {
            continue;
        };
        if let Some((_, key)) = loops.get_mut(list) {
            if key.is_some() || target.contains('.') {
                rejected.insert(list);
            }
            *key = Some(target);
        }
    }
    loops.retain(|list, (_, key)| key.is_some() && !rejected.contains(list));

    // The loans each loop's key tests fuse with.
    let mut test_loans = HashMap::<&str, HashSet<&str>>::new();
    let mut assigned = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            for (place, access) in instruction_accesses(instruction) {
                if access == Access::Assign {
                    *assigned.entry(root(place)).or_default() += 1;
                }
            }
            let roots = mentioned_roots(instruction);
            for (&list, &(dict, key)) in &loops {
                let key = key.expect("kept loops have a key");
                if roots.contains(list) && !uses_keys(instruction, list, dict, key) {
                    rejected.insert(list);
                }
                if roots.contains(key) {
                    match key_test(instruction, dict, key, fused, shadow) {
                        Some(KeyUse::Test(loan)) => {
                            test_loans.entry(list).or_default().insert(loan);
                        }
                        Some(KeyUse::Other) => {}
                        None if uses_keys(instruction, list, dict, key) => {}
                        None => {
                            rejected.insert(list);
                        }
                    }
                }
                if roots.contains(dict) && !reads_dict(instruction, dict) {
                    rejected.insert(list);
                }
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            for (&list, &(_, key)) in &loops {
                if root(place) == list || Some(root(place)) == key {
                    rejected.insert(list);
                }
            }
        }
    }
    // Each loan that selects by the key must be a test's loan, which the
    // test has already lent the value to.
    for instruction in instructions() {
        let Instruction::BeginElementLoan { loan, selector, .. } = instruction else {
            continue;
        };
        for (&list, &(_, key)) in &loops {
            if matches!(selector, Operand::Place(place) if Some(place.as_str()) == key)
                && !test_loans
                    .get(list)
                    .is_some_and(|loans| loans.contains(loan.as_str()))
            {
                rejected.insert(list);
            }
        }
    }
    loops
        .into_iter()
        .filter(|&(list, (dict, key))| {
            let key = key.expect("kept loops have a key");
            !rejected.contains(list)
                && assigned.get(list) == Some(&1)
                && assigned.get(key) == Some(&1)
                && assigned.get(dict).copied().unwrap_or(0) <= 1
        })
        .map(|(list, (dict, key))| {
            (
                list.to_string(),
                KeyLoop {
                    dict: dict.to_string(),
                    key: key.expect("kept loops have a key").to_string(),
                },
            )
        })
        .collect()
}

/// Whether `instruction`, which names the keys copy `list`, is one of the
/// loop's uses of it: the copy itself, its length, the key read, the fact
/// that the read is in range, or its release.
fn uses_keys(instruction: &Instruction, list: &str, dict: &str, key: &str) -> bool {
    match instruction {
        Instruction::Assign { target, value } => match runtime_call(value) {
            Some((RuntimeOp::Dict(DictOp::Keys), receiver, _)) => {
                target == list && receiver == dict
            }
            Some((RuntimeOp::List(ListOp::Len), receiver, args)) => {
                receiver == list && args.is_empty() && root(target) != list
            }
            Some((RuntimeOp::List(ListOp::IndexUnchecked), receiver, args)) => {
                receiver == list
                    && target == key
                    && args.iter().all(|arg| {
                        arg.writeback_place.is_none()
                            && !matches!(
                                &arg.value,
                                Operand::Place(place) | Operand::MovePlace(place)
                                    if root(place) == list || root(place) == key
                            )
                    })
            }
            _ => false,
        },
        Instruction::IndexInRange { list: read, index } => read == list && root(index) != list,
        Instruction::Release { place } => place == list,
        _ => false,
    }
}

/// A use of a loop's key besides the read that makes it.
enum KeyUse<'f> {
    /// A fused `contains_key` test, with the loan it lends the value to.
    Test(&'f str),
    /// The loan of a test, which reads no key, or the key's release.
    Other,
}

/// The use `instruction` makes of `key`, or `None` when it is no use the
/// walk by entry can stand in for.
fn key_test<'f>(
    instruction: &'f Instruction,
    dict: &str,
    key: &str,
    fused: &'f HashMap<String, FusedEntryProbe>,
    shadow: &HashSet<String>,
) -> Option<KeyUse<'f>> {
    match instruction {
        Instruction::Assign { target, value } => {
            let (RuntimeOp::Dict(DictOp::ContainsKey), receiver, [arg]) = runtime_call(value)?
            else {
                return None;
            };
            let probe = fused.get(target)?;
            (receiver == dict
                && arg.writeback_place.is_none()
                && matches!(&arg.value, Operand::Place(place) if place == key)
                && !probe.mutable
                && !probe.lent
                && shadow.contains(&probe.loan))
            .then_some(KeyUse::Test(probe.loan.as_str()))
        }
        Instruction::BeginElementLoan {
            source,
            selector: Operand::Place(selector),
            mutable: false,
            ..
        } if source == dict && selector == key => Some(KeyUse::Other),
        Instruction::Release { place } if place == key => Some(KeyUse::Other),
        _ => None,
    }
}

/// Whether `instruction`, which names `dict`, leaves it as it is: it reads
/// the dict, tests or lends one of its entries, assigns it, or releases it.
fn reads_dict(instruction: &Instruction, dict: &str) -> bool {
    match instruction {
        Instruction::Release { place } => place == dict,
        Instruction::BeginElementLoan {
            source,
            mutable: false,
            loan,
            ..
        } => source == dict && loan != dict,
        Instruction::Assign {
            value: Rvalue::Closure { .. } | Rvalue::StartTask { .. },
            ..
        } => false,
        Instruction::Assign { target, value } => {
            // The dict's one assignment makes it.
            if target == dict {
                return true;
            }
            if root(target) == dict {
                return false;
            }
            // A read of the dict names it as its receiver's writeback.
            let reads_receiver = matches!(
                runtime_call(value),
                Some((RuntimeOp::Dict(op), receiver, _)) if receiver == dict && matches!(
                    op,
                    DictOp::Len
                        | DictOp::IsEmpty
                        | DictOp::Get
                        | DictOp::GetStr
                        | DictOp::GetInt64
                        | DictOp::ContainsKey
                        | DictOp::ContainsKeyInt64
                        | DictOp::Keys
                        | DictOp::Values
                        | DictOp::Items
                        | DictOp::Index
                )
            );
            instruction_accesses(instruction)
                .into_iter()
                .filter(|(place, _)| root(place) == dict)
                .all(|(place, access)| {
                    access == Access::Read
                        || (access == Access::Writeback && place == dict && reads_receiver)
                })
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "native_codegen_key_loops_tests.rs"]
mod tests;
