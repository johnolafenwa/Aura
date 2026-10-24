//! Finds the `Lookup` temporaries that native code keeps as the found
//! value's box alone.
//!
//! `match table.get(key)` lowers to a temporary that holds a boxed
//! `Lookup`, a match on its variant, and a copy of its payload. When the
//! temporary is used for nothing else, the code generator stores the
//! payload's box in it instead, or null for `Lookup.Missing`. The match
//! then tests the word for null, and the payload is a new reference to the
//! same box. No `Lookup` is built, tagged, matched by name, or copied.

use std::collections::{HashMap, HashSet};

use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MirFunction,
    Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{DictOp, RuntimeOp};

/// Whether `value` looks a key up in a `dict` with `get`.
pub(super) fn is_dict_get(value: &Rvalue) -> bool {
    matches!(
        value,
        Rvalue::Call {
            callee: CallTarget::Runtime {
                op: RuntimeOp::Dict(DictOp::Get | DictOp::GetStr),
                ..
            },
            ..
        }
    )
}

/// The temporaries of `function` that may hold a lookup's payload in place
/// of the `Lookup`. Each one is a compiler temporary that is assigned only
/// from `get` and is otherwise only matched on its `Lookup` variant,
/// unpacked in one place at most once per lookup, and released.
pub(super) fn lookup_payload_slots(function: &MirFunction) -> HashSet<String> {
    let mut candidates = HashMap::<&str, Candidate>::new();
    let mut rejected = HashSet::<&str>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Instruction::Assign { target, value } = instruction {
                if target.starts_with("%t") && !target.contains('.') && is_dict_get(value) {
                    candidates
                        .entry(target)
                        .or_default()
                        .assigned
                        .insert(&block.label);
                } else {
                    rejected.insert(target);
                }
            }
        }
    }
    for param in &function.params {
        rejected.insert(&param.name);
    }
    candidates.retain(|name, _| !rejected.contains(name));
    if candidates.is_empty() {
        return HashSet::new();
    }

    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::Release { .. } => continue,
                Instruction::Assign {
                    value:
                        Rvalue::VariantPayload {
                            scrutinee: Operand::Place(place) | Operand::MovePlace(place),
                            index,
                            ..
                        },
                    ..
                } if candidates.contains_key(place.as_str()) => {
                    let candidate = candidates.get_mut(place.as_str()).expect("a candidate");
                    if *index == 0 {
                        candidate.unpacked_in.push(&block.label);
                    } else {
                        rejected.insert(place);
                    }
                    continue;
                }
                _ => {}
            }
            let mut mentions = Vec::new();
            instruction_place_mentions(instruction, &mut mentions);
            if let Instruction::ReadLoan { target, .. } = instruction {
                mentions.push(target);
            }
            rejected.extend(
                mentions
                    .into_iter()
                    .filter(|place| candidates.contains_key(place)),
            );
        }
        match &block.terminator {
            Terminator::Match {
                scrutinee: Operand::Place(place),
                arms,
                ..
            } if candidates.contains_key(place.as_str()) => {
                let tests_lookup = arms.iter().all(|arm| {
                    arm.wildcard
                        || (matches!(arm.enum_name.as_deref(), None | Some("Lookup"))
                            && matches!(arm.variant_name.as_deref(), Some("Found" | "Missing")))
                });
                if !tests_lookup {
                    rejected.insert(place);
                }
            }
            terminator => rejected.extend(
                terminator_place_mentions(terminator)
                    .into_iter()
                    .filter(|place| candidates.contains_key(place)),
            ),
        }
    }
    candidates.retain(|name, _| !rejected.contains(name));

    let successors = function
        .blocks
        .iter()
        .map(|block| (block.label.as_str(), successors(&block.terminator)))
        .collect::<HashMap<_, _>>();
    candidates
        .into_iter()
        .filter(|(_, candidate)| match candidate.unpacked_in.as_slice() {
            [] => true,
            [block] => !reaches_again_without_assignment(block, &candidate.assigned, &successors),
            // Two copies of one payload would share one box.
            _ => false,
        })
        .map(|(name, _)| name.to_string())
        .collect()
}

#[derive(Default)]
struct Candidate<'a> {
    /// The blocks that assign the temporary.
    assigned: HashSet<&'a str>,
    /// The blocks that copy its payload.
    unpacked_in: Vec<&'a str>,
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

/// Whether control can leave `start` and come back to it without passing
/// a block that assigns the temporary. A payload copied twice from one
/// lookup would share one box, so a change to the first copy would show in
/// the second.
fn reaches_again_without_assignment(
    start: &str,
    assigned: &HashSet<&str>,
    successors: &HashMap<&str, Vec<&str>>,
) -> bool {
    let mut seen = HashSet::new();
    let mut pending = successors.get(start).cloned().unwrap_or_default();
    while let Some(block) = pending.pop() {
        if block == start {
            return true;
        }
        if assigned.contains(block) || !seen.insert(block) {
            continue;
        }
        pending.extend(successors.get(block).into_iter().flatten().copied());
    }
    false
}
