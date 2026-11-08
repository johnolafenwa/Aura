//! Finds the entry loans whose one read is matched and whose payloads are
//! only copied, which a shadow box serves instead of a copy of the entry.
//!
//! `match fields.lookup(name): case Found(value): match value: case
//! json.Value.String(text): return text.clone()` lowers to a fused
//! `contains_key` test, an entry loan, one `ReadLoan` of it into a local,
//! a `Match` on that local, a `VariantPayload` per arm that binds one, and
//! a clone of the payload. The read used to copy the whole entry, so
//! reading one string of a JSON object copied the object's value tree.
//!
//! When the local is only matched, read for payloads, and released, and
//! each payload is only cloned and released on a straight path that runs
//! nothing else, the fused test may give the read a shadow box (see
//! `aura_direct_map_get_shadow`): the box reads the entry where the dict
//! keeps it. The match and the payload reads come before the loan ends, so
//! the shadow is live for them. A payload holds its own reference to the
//! payload box, and the path to its last use only ends loans, releases,
//! and clones it, so nothing changes the entry while it is read. The
//! loan's end takes the shadow back, and the local's own release is
//! skipped, since the local never owned the box.

use std::collections::{HashMap, HashSet};

use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, BasicBlock, CallTarget, Instruction,
    MirFunction, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{RuntimeOp, ValueOp};

/// The loans of `entry_copies` (see `element_loans::entry_copy_loans`),
/// besides those `lent` holds, whose one read is a local only matched and
/// read for payloads that are only cloned, by loan, with that local.
pub(super) fn shadow_read_loans(
    function: &MirFunction,
    entry_copies: &HashMap<String, bool>,
    lent: &HashSet<String>,
) -> HashMap<String, String> {
    let mut mentions = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        let mut places = Vec::new();
        for instruction in &block.instructions {
            instruction_place_mentions(instruction, &mut places);
        }
        places.extend(terminator_place_mentions(&block.terminator));
        for place in places {
            *mentions.entry(root(place)).or_default() += 1;
        }
    }
    let labels = function
        .blocks
        .iter()
        .map(|block| (block.label.as_str(), block))
        .collect::<HashMap<_, _>>();
    let mut loans = HashMap::new();
    for block in &function.blocks {
        // The read is the block's last instruction, and the block's match
        // tests the local it reads into.
        let Some(Instruction::ReadLoan { target, loan }) = block.instructions.last() else {
            continue;
        };
        if entry_copies.get(loan.as_str()) != Some(&true) || lent.contains(loan.as_str()) {
            continue;
        }
        let Terminator::Match {
            scrutinee: Operand::Place(scrutinee),
            arms,
            otherwise,
        } = &block.terminator
        else {
            continue;
        };
        if scrutinee != target || target.contains('.') {
            continue;
        }
        // The read and the match name the local, and each arm may read a
        // payload of it and release it.
        let mut uses = 2;
        let mut payloads_ok = true;
        let mut arm_labels = arms
            .iter()
            .map(|arm| arm.label.as_str())
            .collect::<HashSet<_>>();
        arm_labels.insert(otherwise.as_str());
        for other in &function.blocks {
            for (position, instruction) in other.instructions.iter().enumerate() {
                match instruction {
                    Instruction::Release { place } if place == target => {
                        uses += names(instruction, target);
                    }
                    Instruction::Assign {
                        target: payload,
                        value:
                            Rvalue::VariantPayload {
                                scrutinee: Operand::Place(source),
                                ..
                            },
                    } if source == target => {
                        uses += names(instruction, target);
                        payloads_ok &= arm_labels.contains(other.label.as_str())
                            && !ends_loan(&other.instructions[..position], loan)
                            && payload_only_cloned(payload, other, position, &labels, &mentions);
                    }
                    _ => {}
                }
            }
        }
        if payloads_ok && mentions.get(target.as_str()) == Some(&uses) {
            loans.insert(loan.clone(), target.clone());
        }
    }
    loans
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// How many times `instruction` names the root `local`.
fn names(instruction: &Instruction, local: &str) -> usize {
    let mut places = Vec::new();
    instruction_place_mentions(instruction, &mut places);
    places
        .into_iter()
        .filter(|place| root(place) == local)
        .count()
}

fn ends_loan(instructions: &[Instruction], loan: &str) -> bool {
    instructions.iter().any(
        |instruction| matches!(instruction, Instruction::EndLoan { loan: ended } if ended == loan),
    )
}

/// Whether `payload`, which instruction `position` of `block` assigns,
/// is named only by clones of it and its release on the straight path from
/// there, where every other instruction ends a loan or releases a place.
fn payload_only_cloned(
    payload: &str,
    block: &BasicBlock,
    position: usize,
    labels: &HashMap<&str, &BasicBlock>,
    mentions: &HashMap<&str, usize>,
) -> bool {
    let mut uses = 0;
    let mut current = block;
    let mut start = position + 1;
    let mut visited = HashSet::new();
    loop {
        for instruction in &current.instructions[start..] {
            match instruction {
                Instruction::Release { place } if place == payload => {
                    uses += names(instruction, payload);
                    return mentions.get(payload) == Some(&uses);
                }
                Instruction::Release { .. } | Instruction::EndLoan { .. } => {}
                Instruction::Assign {
                    target,
                    value:
                        Rvalue::Call {
                            callee:
                                CallTarget::Runtime {
                                    op: RuntimeOp::Value(ValueOp::Clone),
                                    receiver: Some(Operand::Place(receiver)),
                                    ..
                                },
                            args,
                            ..
                        },
                } if receiver == payload && args.is_empty() && root(target) != payload => {
                    uses += names(instruction, payload);
                }
                _ => return false,
            }
        }
        let Terminator::Goto(next) = &current.terminator else {
            return false;
        };
        if !visited.insert(next.as_str()) {
            return false;
        }
        let Some(next) = labels.get(next.as_str()) else {
            return false;
        };
        current = next;
        start = 0;
    }
}

#[cfg(test)]
#[path = "native_codegen_shadow_reads_tests.rs"]
mod tests;
