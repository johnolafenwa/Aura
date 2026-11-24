//! Finds the loops over a dict's items, keys, or values that native code
//! walks by entry instead of through a list.
//!
//! `for key, value in d.items()` lowers to a list of every entry as a
//! tuple, a copy of each tuple read out of it, and a copy of each tuple's
//! two elements. `d.keys()` and `d.values()` make a list of copies too.
//! Native code makes none of the lists. In place of the list it takes a
//! snapshot of the dict, a second handle to the same entries (see
//! `aura_direct_map_snapshot`), and walks the snapshot's entries by slot,
//! copying out each key or value where the loop reads it.
//!
//! The snapshot keeps what the list promised: the loop sees the entries
//! the dict held when the loop began, in order, whatever the body does to
//! the dict. Dicts copy their entries on the first write through a handle
//! that shares them, so a body that changes the dict writes to entries of
//! its own.
//!
//! A loop qualifies when the list is a compiler temporary that is only
//! made, measured, indexed by the loop, and released, and when what it
//! holds are plain values: numbers, `bool`s, and `str`s, whose copies
//! share nothing with the dict. For `items`, the tuple read out of the
//! list must only be taken apart and released.

use std::collections::{HashMap, HashSet};

use aura_core::types::Type;

use crate::mir::ownership::{instruction_accesses, Access};
use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MirFunction,
    Operand, Rvalue,
};
use crate::runtime_ops::{DictOp, ListOp, RuntimeOp};

/// What a walked loop reads from each entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EntryPart {
    /// `d.keys()`: the key.
    Key,
    /// `d.values()`: the value.
    Value,
    /// `d.items()`: a tuple the loop takes apart.
    Both,
}

/// One loop native code walks by entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EntryLoop {
    pub(super) part: EntryPart,
    /// The local each list element is read into: the key, the value, or
    /// the tuple of both.
    pub(super) element: String,
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

fn is_temporary(place: &str) -> bool {
    place.starts_with("%t") && !place.contains('.')
}

/// Whether a copy of a value of type `ty` shares nothing with the value.
fn is_plain(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Named(name, args) if args.is_empty() && matches!(
            name.as_str(),
            "int8" | "int16" | "int32" | "int64"
                | "uint8" | "uint16" | "uint32" | "uint64"
                | "float32" | "float64" | "bool" | "str"
        )
    )
}

/// The part of each entry a list of `element` made by `op` holds, when
/// the loop over it can walk the entries instead.
fn walked_part(op: DictOp, element: &Type) -> Option<EntryPart> {
    match (op, element) {
        (DictOp::Keys, element) if is_plain(element) => Some(EntryPart::Key),
        (DictOp::Values, element) if is_plain(element) => Some(EntryPart::Value),
        (DictOp::Items, Type::Tuple(parts)) if parts.len() == 2 && parts.iter().all(is_plain) => {
            Some(EntryPart::Both)
        }
        _ => None,
    }
}

/// The op and receiver of a runtime call, with its arguments.
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
        } => Some((*op, receiver.as_str(), args.as_slice())),
        _ => None,
    }
}

/// The roots `instruction` names, as targets, operands, or places it reads.
pub(super) fn mentioned_roots(instruction: &Instruction) -> HashSet<&str> {
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

/// Whether `argument` names neither `list` nor `element`.
fn names_neither(argument: &crate::mir::MirArg, list: &str, element: &str) -> bool {
    argument.writeback_place.is_none()
        && !matches!(
            &argument.value,
            Operand::Place(place) | Operand::MovePlace(place)
                if root(place) == list || root(place) == element
        )
}

/// How `instruction`, which names the list `list`, uses it.
enum ListUse<'f> {
    /// The call that makes it, its length, the fact that a read is in
    /// range, or its release.
    Plain,
    /// The loop's read of one element into this local.
    Read(&'f str),
}

fn list_use<'f>(instruction: &'f Instruction, list: &str) -> Option<ListUse<'f>> {
    match instruction {
        Instruction::Assign { target, value } => match runtime_call(value)? {
            (RuntimeOp::Dict(DictOp::Keys | DictOp::Values | DictOp::Items), receiver, []) => {
                (target == list && root(receiver) != list).then_some(ListUse::Plain)
            }
            (RuntimeOp::List(ListOp::Len), receiver, []) => {
                (receiver == list && root(target) != list).then_some(ListUse::Plain)
            }
            (
                RuntimeOp::List(ListOp::IndexUnchecked | ListOp::IndexScalarUnchecked(_)),
                receiver,
                args,
            ) => (receiver == list
                && !target.contains('.')
                && target != list
                && args.iter().all(|arg| names_neither(arg, list, target)))
            .then_some(ListUse::Read(target)),
            _ => None,
        },
        Instruction::IndexInRange { list: read, index } => {
            (read == list && root(index) != list).then_some(ListUse::Plain)
        }
        Instruction::Release { place } => (place == list).then_some(ListUse::Plain),
        _ => None,
    }
}

/// Whether `instruction`, which names the tuple `element` of an `items`
/// loop, only takes it apart or releases it.
fn takes_apart(instruction: &Instruction, list: &str, element: &str) -> bool {
    match instruction {
        Instruction::Assign {
            target,
            value:
                Rvalue::TupleElement {
                    tuple: Operand::Place(tuple),
                    index,
                    ..
                },
        } => tuple == element && *index < 2 && root(target) != element && root(target) != list,
        Instruction::Release { place } => place == element,
        _ => false,
    }
}

/// The loops of `function` native code walks by entry, by the temporary
/// that would hold the list. `element_type` gives the element type of a
/// local that holds a list.
pub(super) fn entry_loops(
    function: &MirFunction,
    element_type: impl Fn(&str) -> Option<Type>,
) -> HashMap<String, EntryLoop> {
    let instructions = || function.blocks.iter().flat_map(|block| &block.instructions);
    let mut parts = HashMap::<&str, EntryPart>::new();
    let mut rejected = HashSet::<&str>::new();
    for instruction in instructions() {
        let Instruction::Assign { target, value } = instruction else {
            continue;
        };
        let Some((RuntimeOp::Dict(op), _, [])) = runtime_call(value) else {
            continue;
        };
        if !is_temporary(target) {
            continue;
        }
        let Some(part) = element_type(target).and_then(|element| walked_part(op, &element)) else {
            continue;
        };
        if parts.insert(target, part).is_some() {
            rejected.insert(target);
        }
    }
    if parts.is_empty() {
        return HashMap::new();
    }

    let mut elements = HashMap::<&str, &str>::new();
    let mut assigned = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            for (place, access) in instruction_accesses(instruction) {
                if access == Access::Assign {
                    *assigned.entry(root(place)).or_default() += 1;
                }
            }
            let roots = mentioned_roots(instruction);
            for &list in parts.keys() {
                if !roots.contains(list) {
                    continue;
                }
                match list_use(instruction, list) {
                    Some(ListUse::Plain) => {}
                    Some(ListUse::Read(element)) => {
                        if elements.insert(list, element).is_some() {
                            rejected.insert(list);
                        }
                    }
                    None => {
                        rejected.insert(list);
                    }
                }
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            if parts.contains_key(root(place)) {
                rejected.insert(root(place));
            }
        }
    }
    // The tuple of an `items` loop is never made, so nothing may use it
    // whole. One local read from two lists is neither's.
    let mut readers = HashMap::<&str, usize>::new();
    for element in elements.values() {
        *readers.entry(element).or_default() += 1;
    }
    let params = function
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<HashSet<_>>();
    for (&list, &element) in &elements {
        if readers[element] != 1 || params.contains(element) || parts.contains_key(element) {
            rejected.insert(list);
        }
    }
    for block in &function.blocks {
        for (&list, &element) in &elements {
            if parts[list] != EntryPart::Both {
                continue;
            }
            for instruction in &block.instructions {
                if mentioned_roots(instruction).contains(element)
                    && !matches!(list_use(instruction, list), Some(ListUse::Read(_)))
                    && !takes_apart(instruction, list, element)
                {
                    rejected.insert(list);
                }
            }
            if terminator_place_mentions(&block.terminator)
                .into_iter()
                .any(|place| root(place) == element)
            {
                rejected.insert(list);
            }
        }
    }
    parts
        .into_iter()
        .filter_map(|(list, part)| {
            let element = *elements.get(list)?;
            (!rejected.contains(list)
                && assigned.get(list) == Some(&1)
                && assigned.get(element) == Some(&1))
            .then(|| {
                (
                    list.to_string(),
                    EntryLoop {
                        part,
                        element: element.to_string(),
                    },
                )
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "native_codegen_entry_loops_tests.rs"]
mod tests;
