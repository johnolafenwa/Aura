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
//!
//! A read of a `json.Value` whose payloads are a `String` cloned once or
//! an `Array` only asked for its length is a node read (see
//! [`node_read_loans`]): the fused test may give it the address of the
//! entry's node in a parsed document's tape instead of a box (see
//! `aura_direct_map_get_node_shadow`). The match tests the node's tag, and
//! each payload read makes what its one use asks for, the copy of the text
//! or the length, from the node, so a field read of a parsed document
//! makes no box for the entry, no list, and one copy of a string.

use std::collections::{HashMap, HashSet};

use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, BasicBlock, CallTarget, Instruction,
    MirFunction, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{ListOp, RuntimeOp, ValueOp};

use super::{
    types, DirectType, FunctionCompiler, IntCC, MemFlags, RuntimeFn, ScalarKind, Type, Val,
    ValueRef,
};

/// What generated code makes of a payload of a node read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodePayload {
    /// A `String`'s text, which is cloned once: the read makes the clone.
    Text,
    /// An `Array`'s list, which is only asked for its length: the read
    /// makes the length.
    Len,
}

/// A loan whose one read a tape node may serve.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NodeRead {
    /// The local the loan is read into.
    pub(super) target: String,
    /// The locals the read's payloads are bound to.
    pub(super) payloads: HashMap<String, NodePayload>,
}

/// How a payload is used on the straight path from its read to its
/// release.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PayloadUses {
    clones: usize,
    lens: usize,
}

/// The loans of `entry_copies` (see `element_loans::entry_copy_loans`),
/// besides those `lent` holds, whose one read is a local only matched and
/// read for payloads that are only cloned, by loan, with that local.
pub(super) fn shadow_read_loans(
    function: &MirFunction,
    entry_copies: &HashMap<String, bool>,
    lent: &HashSet<String>,
) -> HashMap<String, String> {
    matched_read_loans(function, entry_copies, lent, |_, _, uses| {
        (uses.lens == 0).then_some(())
    })
    .into_iter()
    .map(|(loan, (target, _))| (loan, target))
    .collect()
}

/// The loans [`shadow_read_loans`] would find if a payload might also be
/// asked for its length, whose every payload is a `String`'s that is
/// cloned once or an `Array`'s that is only asked for its length, by loan.
/// The caller checks that the loan reads a `json.Value`.
pub(super) fn node_read_loans(
    function: &MirFunction,
    entry_copies: &HashMap<String, bool>,
    lent: &HashSet<String>,
) -> HashMap<String, NodeRead> {
    matched_read_loans(function, entry_copies, lent, |variant, index, uses| match (
        variant,
        index,
        uses.clones,
        uses.lens,
    ) {
        ("String", 0, 1, 0) => Some(NodePayload::Text),
        ("Array", 0, 0, lens) if lens > 0 => Some(NodePayload::Len),
        _ => None,
    })
    .into_iter()
    .map(|(loan, (target, payloads))| {
        let payloads = payloads.into_iter().collect();
        (loan, NodeRead { target, payloads })
    })
    .collect()
}

/// The loans of `entry_copies`, besides those `lent` holds, whose one read
/// is a local only matched and read for payloads, each of which is only
/// cloned or asked for its length before its release and for which
/// `payload` gives a value from its variant's name, its index, and those
/// uses. By loan, with the local and each payload local's value.
fn matched_read_loans<T>(
    function: &MirFunction,
    entry_copies: &HashMap<String, bool>,
    lent: &HashSet<String>,
    payload: impl Fn(&str, usize, PayloadUses) -> Option<T>,
) -> HashMap<String, (String, Vec<(String, T)>)> {
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
        let mut payloads = Vec::new();
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
                        target: bound,
                        value:
                            Rvalue::VariantPayload {
                                scrutinee: Operand::Place(source),
                                variant_name,
                                index,
                            },
                    } if source == target => {
                        uses += names(instruction, target);
                        let read = (arm_labels.contains(other.label.as_str())
                            && !ends_loan(&other.instructions[..position], loan))
                        .then(|| payload_uses(bound, other, position, &labels, &mentions))
                        .flatten()
                        .and_then(|uses| payload(variant_name, *index, uses));
                        match read {
                            Some(read) => payloads.push((bound.clone(), read)),
                            None => payloads_ok = false,
                        }
                    }
                    _ => {}
                }
            }
        }
        if payloads_ok && mentions.get(target.as_str()) == Some(&uses) {
            loans.insert(loan.clone(), (target.clone(), payloads));
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

/// How `payload`, which instruction `position` of `block` assigns, is
/// used, when it is named only by clones of it, reads of its length, and
/// its release on the straight path from there, where every other
/// instruction ends a loan or releases a place. `None` for any other use.
fn payload_uses(
    payload: &str,
    block: &BasicBlock,
    position: usize,
    labels: &HashMap<&str, &BasicBlock>,
    mentions: &HashMap<&str, usize>,
) -> Option<PayloadUses> {
    let mut named = 0;
    let mut uses = PayloadUses::default();
    let mut current = block;
    let mut start = position + 1;
    let mut visited = HashSet::new();
    loop {
        for instruction in &current.instructions[start..] {
            match instruction {
                Instruction::Release { place } if place == payload => {
                    named += names(instruction, payload);
                    return (mentions.get(payload) == Some(&named)).then_some(uses);
                }
                Instruction::Release { .. } | Instruction::EndLoan { .. } => {}
                Instruction::Assign {
                    target,
                    value:
                        Rvalue::Call {
                            callee:
                                CallTarget::Runtime {
                                    op,
                                    receiver: Some(Operand::Place(receiver)),
                                    ..
                                },
                            args,
                            ..
                        },
                } if receiver == payload && args.is_empty() && root(target) != payload => {
                    match op {
                        RuntimeOp::Value(ValueOp::Clone) => uses.clones += 1,
                        RuntimeOp::List(ListOp::Len) => uses.lens += 1,
                        _ => return None,
                    }
                    named += names(instruction, payload);
                }
                _ => return None,
            }
        }
        let Terminator::Goto(next) = &current.terminator else {
            return None;
        };
        if !visited.insert(next.as_str()) {
            return None;
        }
        current = labels.get(next.as_str())?;
        start = 0;
    }
}

/// The payload local whose clone or length `rvalue` reads, with which.
pub(super) fn payload_read(rvalue: &Rvalue) -> Option<(&str, NodePayload)> {
    let Rvalue::Call {
        callee:
            CallTarget::Runtime {
                op,
                receiver: Some(Operand::Place(receiver)),
                ..
            },
        args,
        ..
    } = rvalue
    else {
        return None;
    };
    let read = match op {
        RuntimeOp::Value(ValueOp::Clone) => NodePayload::Text,
        RuntimeOp::List(ListOp::Len) => NodePayload::Len,
        _ => return None,
    };
    args.is_empty().then_some((receiver.as_str(), read))
}

impl FunctionCompiler<'_> {
    /// Tests the word of a node read's local (see [`node_read_loans`]) for
    /// variant `variant_name` of `json.Value`: by its node's tag when the
    /// word is a tape node's address, and with `boxed` when it is a box.
    pub(super) fn node_read_variant_test(
        &mut self,
        word: Val,
        variant_name: &str,
        boxed: impl FnOnce(&mut Self) -> std::result::Result<Val, String>,
    ) -> std::result::Result<Val, String> {
        let node_block = self.builder.create_block();
        let box_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        let bit = self.builder.band_imm(word, 2);
        self.builder.brif(bit, node_block, &[], box_block, &[]);

        self.builder.switch_to_block(node_block);
        self.builder.seal_block(node_block);
        let matched = match Self::json_tape_tags(variant_name) {
            Some(tags) => {
                let node = self.builder.iadd_imm(word, -2);
                let matched = self.json_node_tag_in(node, tags);
                self.builder.uextend(types::I64, matched)
            }
            None => self.builder.iconst(types::I64, 0),
        };
        self.builder.jump(join_block, &[matched]);

        self.builder.switch_to_block(box_block);
        self.builder.seal_block(box_block);
        let matched = boxed(self)?;
        self.builder.jump(join_block, &[matched]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(self.builder.block_params(join_block)[0])
    }

    /// Reads payload local `payload` of a node read from `source`, the
    /// local the loan was read into: one call makes the copy of a
    /// `String`'s text or the length of an `Array`'s list, which the
    /// payload's one kind of use then takes (see
    /// [`Self::take_node_payload`]). The payload itself is never made, and
    /// its release releases nothing.
    pub(super) fn read_node_payload(
        &mut self,
        payload: &str,
        source: &str,
    ) -> std::result::Result<(), String> {
        let (read, loan) = self.node_payloads[payload].clone();
        let word = self.load_root(source)?.values[0];
        let made = match read {
            NodePayload::Text => {
                // A node's text lies in the tape of the document the fused
                // test looked up, which the loan keeps alive.
                let map = self.builder.use_var(self.node_read_maps[&loan]);
                self.builder
                    .call(self.runtime[RuntimeFn::JsonStringCopy], &[map, word])
                    .results[0]
            }
            NodePayload::Len => {
                self.builder
                    .call(self.runtime[RuntimeFn::JsonArrayLen], &[word])
                    .results[0]
            }
        };
        self.builder
            .def_var(self.node_payload_values[payload], made);
        Ok(())
    }

    /// The value of `rvalue` when it clones or measures a payload of a
    /// node read: what [`Self::read_node_payload`] made for it.
    pub(super) fn take_node_payload(
        &mut self,
        rvalue: &Rvalue,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some((payload, read)) = payload_read(rvalue) else {
            return Ok(None);
        };
        if self.node_payloads.get(payload).map(|(read, _)| *read) != Some(read) {
            return Ok(None);
        }
        let made = self.builder.use_var(self.node_payload_values[payload]);
        Ok(Some(match read {
            // The one clone takes the copy.
            NodePayload::Text => self.owned_opaque_result(vec![made], Type::named("str")),
            NodePayload::Len => ValueRef {
                values: vec![made],
                ty: DirectType::Scalar(ScalarKind::Int64),
            },
        }))
    }

    /// Whether the node of a `json.Value` at address `node` in a tape has a
    /// tag in `tags`, as a flag.
    pub(super) fn json_node_tag_in(&mut self, node: Val, tags: (i64, i64)) -> Val {
        let tag = self.builder.uload8(
            types::I64,
            MemFlags::trusted(),
            node,
            aura_runtime::json_tape::NODE_TAG_OFFSET,
        );
        if tags.0 == tags.1 {
            return self.builder.icmp_imm(IntCC::Equal, tag, tags.0);
        }
        let from_first = self.builder.iadd_imm(tag, -tags.0);
        self.builder
            .icmp_imm(IntCC::UnsignedLessThanOrEqual, from_first, tags.1 - tags.0)
    }
}

#[cfg(test)]
#[path = "native_codegen_shadow_reads_tests.rs"]
mod tests;
