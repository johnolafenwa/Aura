//! Finds the list element reads native code lends instead of copying.
//!
//! `xs[i]` on a list of handles reads the element into a local that holds
//! a box. Native code used to copy the element into a new box. A local
//! assigned once from such a read, and otherwise only read, holds the box
//! the list keeps the element in instead (see `aura_direct_vec_lend`), with
//! a flag that says the local does not own it, so its release releases
//! nothing. `str` elements keep the copying read, which their list view
//! makes cheap.
//!
//! The box lives as long as the list keeps it. A callee reaches a local
//! list or a parameter only when it is passed in, so only an instruction
//! that names the list can change, free, copy, or share it. Before each
//! such instruction that does more than read the list, a lent local that
//! is still to be read copies the element into a box of its own, so the
//! list's box is reached only while the list is unchanged (see
//! `native_element_boxes.rs` in the runtime). A list that a closure, a
//! task, or a mutable loan can reach is never lent from.

use std::collections::{HashMap, HashSet};

use crate::mir::ownership::{instruction_accesses, Access};
use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, BasicBlock, CallTarget, Instruction,
    MirFunction, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{DictOp, ListOp, RuntimeOp, SetOp};

/// The lent element reads of one function.
#[derive(Debug, Default)]
pub(super) struct LentElements {
    /// Each local that holds a lent element, with the list it reads.
    pub(super) lists: HashMap<String, String>,
    /// The lent locals that copy their element before the instruction at
    /// each block label and index.
    pub(super) promotions: HashMap<(String, usize), Vec<String>>,
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// The list a lendable element read reads: `List(Index)` or
/// `List(IndexUnchecked)` on a root local.
fn element_read_list(value: &Rvalue) -> Option<&str> {
    match value {
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::List(ListOp::Index | ListOp::IndexUnchecked),
                    receiver: Some(Operand::Place(list)),
                    ..
                },
            ..
        } if !list.contains('.') => Some(list),
        _ => None,
    }
}

/// Whether `instruction` only reads `list`, which a lent element of it
/// outlives.
fn only_reads_list(instruction: &Instruction, list: &str) -> bool {
    match instruction {
        Instruction::IndexInRange { list: read, index } => read == list && root(index) != list,
        Instruction::Assign {
            target,
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::List(op),
                            receiver: Some(Operand::Place(receiver)),
                            ..
                        },
                    args,
                    ..
                },
        } => {
            receiver == list
                && root(target) != list
                && reads_list(*op)
                && args.iter().all(|arg| {
                    arg.writeback_place.is_none()
                        && !matches!(
                            &arg.value,
                            Operand::Place(place) | Operand::MovePlace(place) if root(place) == list
                        )
                })
        }
        _ => false,
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

/// Whether `instruction`, which names the lent local `temp`, only reads
/// it or its parts, or is its own release.
fn reads_lent(instruction: &Instruction, temp: &str) -> bool {
    match instruction {
        Instruction::Release { place } => place == temp,
        // A copy of a part, which the consumer that follows owns.
        Instruction::Retain { place } => place != temp,
        // The part is copied into the temporary, which owns the copy.
        Instruction::Lend {
            place,
            mutable: false,
            ..
        }
        | Instruction::EndLend {
            place,
            mutable: false,
            ..
        } => place != temp,
        Instruction::Lend { .. }
        | Instruction::EndLend { .. }
        | Instruction::BeginLoan { .. }
        | Instruction::BeginElementLoan { .. }
        | Instruction::BeginReturnedLoan { .. }
        | Instruction::Reborrow { .. }
        | Instruction::ReborrowElement { .. }
        | Instruction::PushCleanup { .. }
        | Instruction::PopCleanup { .. }
        | Instruction::Assign {
            value: Rvalue::Closure { .. } | Rvalue::StartTask { .. },
            ..
        } => false,
        _ => {
            let receiver = read_receiver(instruction);
            let accesses = instruction_accesses(instruction)
                .into_iter()
                .filter(|(place, _)| root(place) == temp)
                .collect::<Vec<_>>();
            // Consuming a part reads it out of the box: the consumer gets a
            // copy or a reference to the part, never the box.
            !accesses.is_empty()
                && accesses.iter().all(|(place, access)| match access {
                    Access::Read => true,
                    Access::Consume => *place != temp,
                    Access::Writeback => Some(*place) == receiver,
                    Access::Move | Access::TakePart | Access::Assign => false,
                })
        }
    }
}

/// The receiver place of a runtime call that only reads its receiver, which
/// the MIR names as a writeback all the same: a `str` method, which never
/// changes the string, or a read of a list, dict, or set.
fn read_receiver(instruction: &Instruction) -> Option<&str> {
    let Instruction::Assign {
        value:
            Rvalue::Call {
                callee:
                    CallTarget::Runtime {
                        op,
                        receiver_place: Some(place),
                        ..
                    },
                ..
            },
        ..
    } = instruction
    else {
        return None;
    };
    let reads = match op {
        RuntimeOp::Str(_) => true,
        RuntimeOp::List(op) => reads_list(*op),
        RuntimeOp::Dict(op) => matches!(
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
        ),
        RuntimeOp::Set(op) => matches!(
            op,
            SetOp::Len
                | SetOp::IsEmpty
                | SetOp::Contains
                | SetOp::ContainsInt64
                | SetOp::IndexOption
        ),
        _ => false,
    };
    reads.then_some(place.as_str())
}

/// Whether the list op only reads the list.
fn reads_list(op: ListOp) -> bool {
    matches!(
        op,
        ListOp::Len
            | ListOp::IsEmpty
            | ListOp::Index
            | ListOp::IndexUnchecked
            | ListOp::IndexScalar(_)
            | ListOp::IndexScalarUnchecked(_)
            | ListOp::Get
            | ListOp::IndexOption
            | ListOp::Contains
            | ListOp::IndexOf
            | ListOp::Count
    )
}

/// Whether `instruction` lets something other than the function's own
/// instructions reach `list` later: a closure or a task that captures it,
/// or a loan that can write it.
fn shares_list(instruction: &Instruction, list: &str) -> bool {
    match instruction {
        Instruction::Assign {
            value: Rvalue::Closure { .. } | Rvalue::StartTask { .. },
            ..
        } => mentioned_roots(instruction).contains(list),
        Instruction::BeginLoan {
            source, mutable, ..
        }
        | Instruction::BeginElementLoan {
            source, mutable, ..
        }
        | Instruction::Reborrow {
            parent: source,
            mutable,
            ..
        }
        | Instruction::ReborrowElement {
            parent: source,
            mutable,
            ..
        }
        | Instruction::BeginReturnedLoan {
            origin: source,
            mutable,
            ..
        } => *mutable && root(source) == list,
        _ => false,
    }
}

/// The lent element reads of `function`. `lendable` says which locals hold
/// a handle whose element a list may lend and the MIR releases.
pub(super) fn lent_element_reads(
    function: &MirFunction,
    lendable: impl Fn(&str) -> bool,
) -> LentElements {
    let instructions = || function.blocks.iter().flat_map(|block| &block.instructions);
    let mut assigned = HashMap::<&str, usize>::new();
    for instruction in instructions() {
        for (place, access) in instruction_accesses(instruction) {
            if access == Access::Assign {
                *assigned.entry(root(place)).or_default() += 1;
            }
        }
    }
    let params = function
        .params
        .iter()
        .map(|param| param.name.as_str())
        .chain(function.receiver.map(|_| "self"))
        .collect::<HashSet<_>>();
    let mut lists = HashMap::<&str, &str>::new();
    for instruction in instructions() {
        let Instruction::Assign { target, value } = instruction else {
            continue;
        };
        let Some(list) = element_read_list(value) else {
            continue;
        };
        if !target.contains('.')
            && target != list
            && !params.contains(target.as_str())
            && assigned.get(target.as_str()) == Some(&1)
            && lendable(target)
        {
            lists.insert(target, list);
        }
    }
    let mut rejected = HashSet::<&str>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            let roots = mentioned_roots(instruction);
            for (&temp, &list) in &lists {
                let defines = matches!(
                    instruction,
                    Instruction::Assign { target, value }
                        if target == temp && element_read_list(value).is_some()
                );
                if shares_list(instruction, list)
                    || (roots.contains(temp) && !defines && !reads_lent(instruction, temp))
                {
                    rejected.insert(temp);
                }
            }
        }
        // A match reads its scrutinee in place; anything else that names a
        // lent local, such as returning it, would take it.
        let match_scrutinee = match &block.terminator {
            Terminator::Match {
                scrutinee: Operand::Place(place),
                ..
            } => Some(place.as_str()),
            _ => None,
        };
        for place in terminator_place_mentions(&block.terminator) {
            if Some(place) != match_scrutinee && lists.contains_key(root(place)) {
                rejected.insert(root(place));
            }
        }
    }
    lists.retain(|temp, _| !rejected.contains(temp));

    let mut lent = LentElements::default();
    let blocks = function
        .blocks
        .iter()
        .map(|block| (block.label.as_str(), block))
        .collect::<HashMap<_, _>>();
    let mut temps = lists.iter().collect::<Vec<_>>();
    temps.sort();
    for (&temp, &list) in temps {
        let live_in = live_blocks(function, &blocks, temp);
        for block in &function.blocks {
            let mut live = live_at_end(block, &live_in, temp);
            for (index, instruction) in block.instructions.iter().enumerate().rev() {
                live = live_before(instruction, temp, live);
                if live
                    && mentioned_roots(instruction).contains(list)
                    && !only_reads_list(instruction, list)
                {
                    lent.promotions
                        .entry((block.label.clone(), index))
                        .or_default()
                        .push(temp.to_string());
                }
            }
        }
        lent.lists.insert(temp.to_string(), list.to_string());
    }
    for temps in lent.promotions.values_mut() {
        temps.sort();
    }
    lent
}

/// Whether `temp` is still to be read before `instruction`, given whether
/// it is after it. Its assignment and release end what it held.
fn live_before(instruction: &Instruction, temp: &str, live_after: bool) -> bool {
    match instruction {
        Instruction::Release { place } if place == temp => false,
        Instruction::Assign { target, .. } if target == temp => false,
        _ => live_after || mentioned_roots(instruction).contains(temp),
    }
}

/// Whether `temp` is still to be read at the end of `block`: by its
/// terminator, or before one of its successors.
fn live_at_end(block: &BasicBlock, live_in: &HashSet<&str>, temp: &str) -> bool {
    successors(&block.terminator)
        .into_iter()
        .any(|label| live_in.contains(label))
        || terminator_place_mentions(&block.terminator)
            .into_iter()
            .any(|place| root(place) == temp)
}

/// The blocks before which `temp` is still to be read, by a backward pass
/// to a fixed point.
fn live_blocks<'f>(
    function: &'f MirFunction,
    blocks: &HashMap<&'f str, &'f BasicBlock>,
    temp: &str,
) -> HashSet<&'f str> {
    let mut live_in = HashSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for block in function.blocks.iter().rev() {
            let label = block.label.as_str();
            if live_in.contains(label) {
                continue;
            }
            let block = blocks[label];
            let mut live = live_at_end(block, &live_in, temp);
            for instruction in block.instructions.iter().rev() {
                live = live_before(instruction, temp, live);
            }
            if live {
                live_in.insert(label);
                changed = true;
            }
        }
    }
    live_in
}

fn successors(terminator: &Terminator) -> Vec<&str> {
    match terminator {
        Terminator::Goto(label) => vec![label],
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => vec![then_label, else_label],
        Terminator::ForRange {
            body_label,
            exit_label,
            ..
        } => vec![body_label, exit_label],
        Terminator::Match {
            arms, otherwise, ..
        } => arms
            .iter()
            .map(|arm| arm.label.as_str())
            .chain(std::iter::once(otherwise.as_str()))
            .collect(),
        Terminator::Return(_) | Terminator::AssertFail { .. } | Terminator::Unreachable => {
            Vec::new()
        }
    }
}

#[cfg(test)]
#[path = "native_codegen_elements_tests.rs"]
mod tests;
