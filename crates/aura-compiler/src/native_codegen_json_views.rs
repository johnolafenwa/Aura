//! Finds the `Object` payloads of `json.Value` matches that are only looked
//! up, which read the matched value where it lies instead of a dict of
//! their own.
//!
//! `match doc: case json.Value.Object(fields): match fields.lookup(name):
//! ...` lowers to a `VariantPayload` read of `doc` into a local, a fused
//! `contains_key` test on the local, an entry loan of it that copies the
//! entry (see `element_loans::entry_copy_loans`), and the local's release.
//! A parsed document keeps its object in a tape, so the payload read used
//! to build a dict over the tape, tag it, take a reference, and release it
//! again, around a lookup that reads the tape all the same.
//!
//! A local used only this way is a view: it holds the box of the value it
//! was read from, or the dict box that value's variant holds, without a
//! reference, as a lent payload does (see `elements::lent_payload_reads`).
//! The fused test's lookup reads either box (see
//! `aura_direct_map_get_scalar_lent`), and nothing else reads the local, so
//! no code sees that it is not a dict.

use std::collections::{HashMap, HashSet};

use crate::mir::ownership::{instruction_accesses, Access};
use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MirFunction,
    Operand, Rvalue,
};
use crate::runtime_ops::{DictOp, RuntimeOp};

use super::element_loans::FusedEntryProbe;

/// An `Object` payload local that is only looked up.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ObjectView {
    /// The local whose `Object` payload the view reads.
    pub(super) source: String,
    /// The entry loans of the view, each of which a fused test begins.
    pub(super) loans: Vec<String>,
}

/// The locals of `function` that one `Object` payload read of a root local
/// assigns, and nothing else does, and that are otherwise named only by
/// their release, by `contains_key` tests in `probes` whose loan copies its
/// entry (see `entry_copies`), and by the loans those tests begin, of which
/// there is at least one, by local.
pub(super) fn object_view_candidates(
    function: &MirFunction,
    probes: &HashMap<String, FusedEntryProbe>,
    entry_copies: &HashMap<String, bool>,
) -> HashMap<String, ObjectView> {
    // How often each root is named, besides as the target of an assignment,
    // and how often it is assigned.
    let mut mentions = HashMap::<&str, usize>::new();
    let mut assigned = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        let mut places = Vec::new();
        for instruction in &block.instructions {
            instruction_place_mentions(instruction, &mut places);
            for (place, access) in instruction_accesses(instruction) {
                if access == Access::Assign {
                    *assigned.entry(root(place)).or_default() += 1;
                }
            }
        }
        places.extend(terminator_place_mentions(&block.terminator));
        for place in places {
            *mentions.entry(root(place)).or_default() += 1;
        }
    }
    let copies = |probe: &FusedEntryProbe| {
        !probe.mutable && !probe.lent && entry_copies.contains_key(&probe.loan)
    };
    let copied = probes
        .values()
        .filter(|probe| copies(probe))
        .map(|probe| probe.loan.as_str())
        .collect::<HashSet<_>>();
    let instructions = || function.blocks.iter().flat_map(|block| &block.instructions);
    // Each local an `Object` payload read assigns, with the local it reads.
    let mut sources = HashMap::<&str, &str>::new();
    for instruction in instructions() {
        if let Instruction::Assign {
            target,
            value:
                Rvalue::VariantPayload {
                    scrutinee: Operand::Place(source),
                    variant_name,
                    index: 0,
                },
        } = instruction
        {
            if variant_name == "Object"
                && !target.contains('.')
                && !source.contains('.')
                && target != source
            {
                sources.insert(target, source);
            }
        }
    }
    // How often the uses a view may have name each local, and the loans
    // among them.
    let mut uses = HashMap::<&str, usize>::new();
    let mut loans = HashMap::<&str, Vec<&str>>::new();
    for instruction in instructions() {
        let view = match instruction {
            Instruction::Release { place } => place,
            Instruction::Assign {
                target: test,
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op: RuntimeOp::Dict(DictOp::ContainsKey),
                                receiver: Some(Operand::Place(map)),
                                ..
                            },
                        args,
                        ..
                    },
            } if probes.get(test).is_some_and(copies)
                && args.iter().all(|arg| !reads(&arg.value, map)) =>
            {
                map
            }
            Instruction::BeginElementLoan {
                loan,
                source,
                selector,
                projection,
                mutable: false,
                ..
            } if copied.contains(loan.as_str())
                && projection.is_empty()
                && !reads(selector, source) =>
            {
                loans.entry(source).or_default().push(loan);
                source
            }
            _ => continue,
        };
        *uses.entry(view).or_default() += names(instruction, view);
    }
    sources
        .into_iter()
        .filter(|(view, _)| assigned.get(view) == Some(&1) && mentions.get(view) == uses.get(view))
        .filter_map(|(view, source)| {
            let view_loans = loans.remove(view)?;
            Some((
                view.to_string(),
                ObjectView {
                    source: source.to_string(),
                    loans: view_loans.into_iter().map(str::to_string).collect(),
                },
            ))
        })
        .collect()
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// Whether `operand` names the root `local`.
fn reads(operand: &Operand, local: &str) -> bool {
    matches!(
        operand,
        Operand::Place(place) | Operand::MovePlace(place) if root(place) == local
    )
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

#[cfg(test)]
#[path = "native_codegen_json_views_tests.rs"]
mod tests;
