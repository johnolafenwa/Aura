//! Loops over the parts of a text that make no list.
//!
//! `for line in text.split("\n"):` makes a list of every part, a `str`
//! box each, and then reads the list once, front to back. When the list is
//! named by nothing but the loop's own length read, its one element read
//! and its releases, native code makes no list: the length read counts the
//! parts in the text, and the element read finds the next part there and
//! holds it as words (see `str_slots` and `split_parts`), the way a
//! loaned element of a `list[str]` is held. `text.lines()` and
//! `text.split_whitespace()` are walked the same way.
//!
//! The element read is the next part of the walk because the loop's index
//! starts at zero in the block of the split and changes only by the one
//! step that follows the element read, so the reads come in order, one
//! part each.
//!
//! The words point into the text that is walked, so the text must stay as
//! it is from the split to each release of the list: the loop's
//! **region**. A callee reaches a local or a parameter only when it is
//! passed in, so only an instruction that names the text can change or
//! free it. Inside the region nothing names it but its release in the
//! block of the split, which is put off to the releases of the list, and
//! no closure or task of the function captures it. No instruction in the
//! region leaves the function on its own, so a release of the list ends
//! every way out, or the task traps and frees what the function holds as
//! it does anywhere else.

use std::collections::{HashMap, HashSet};

use crate::ast::BinaryOp;
use crate::mir::{
    terminator_place_mentions, CallTarget, Instruction, MirFunction, Operand, Rvalue,
};
use crate::runtime_ops::{ListOp, RuntimeOp, StrOp};

use super::split_parts::{named_places, reads_part, root, Step};
use super::*;

/// A loop over the parts of a text whose list is never made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SplitLoop {
    /// The local the split assigns.
    pub(super) list: String,
    /// The `str` that is walked.
    pub(super) receiver: String,
    /// Whether the receiver is a part of a local, such as a field: the
    /// loop then holds a reference to its box of its own.
    pub(super) held: bool,
    /// The separator of a split, and empty for any other walk.
    pub(super) separator: String,
    /// Which walk, as `aura_direct_str_parts_next_view` counts them.
    pub(super) kind: i64,
    /// The block and position of the split.
    pub(super) split: (String, usize),
    /// The block and position of the length read, and its target.
    pub(super) len: (String, usize),
    pub(super) len_target: String,
    /// The block and position of the element read, and its target.
    pub(super) fetch: (String, usize),
    pub(super) element: String,
    /// The releases of the list.
    pub(super) list_releases: Vec<(String, usize)>,
    /// The release of the receiver inside the region.
    pub(super) receiver_release: Option<(String, usize)>,
}

/// The walk a `str` method makes a list for, with its separator, when a
/// loop can walk the text instead: a split by a literal that is not
/// empty, `lines`, or `split_whitespace`.
fn walk_of(op: RuntimeOp, args: &[MirArg]) -> Option<(i64, String)> {
    use aura_runtime::native_runtime::str_views::{
        STR_PARTS_LINES, STR_PARTS_SPLIT, STR_PARTS_WHITESPACE,
    };
    match (op, args) {
        (
            RuntimeOp::Str(StrOp::Split),
            [MirArg {
                value: Operand::String(separator),
                ..
            }],
        ) if !separator.is_empty() => Some((STR_PARTS_SPLIT, separator.clone())),
        (RuntimeOp::Str(StrOp::Lines), []) => Some((STR_PARTS_LINES, String::new())),
        (RuntimeOp::Str(StrOp::SplitWhitespace), []) => Some((STR_PARTS_WHITESPACE, String::new())),
        _ => None,
    }
}

/// The loops of `function` that walk a text with no list (see the module
/// documentation). Each is a split of a `str` local or parameter whose
/// list is named only by one length read in the block of the split, one
/// element read at the loop's index, and releases. The index is set to
/// zero in the block of the split, raised by one right after the element
/// read, and otherwise only compared. The element is named only by
/// instructions that read it (see [`reads_part`]) and by its releases,
/// which release the box a read made of it, all inside the region, which
/// holds no `try` and names the receiver only by one release in the block
/// of the split. A receiver that is a part of a local, such as a field,
/// is held by the loop, and neither its local nor a place that local
/// borrows from is named in the region.
pub(super) fn split_loops(function: &MirFunction) -> Vec<SplitLoop> {
    let text = Type::named("str");
    let texts = Type::Named("list".to_string(), vec![text.clone()]);
    let is_param = |name: &str| function.params.iter().any(|param| param.name == name);
    let local_is = |name: &str, ty: &Type| {
        !name.contains('.')
            && !is_param(name)
            && function
                .local_types
                .iter()
                .any(|local| local.name == name && local.ty == *ty)
    };
    let block_of = |label: &str| {
        function
            .blocks
            .iter()
            .position(|block| block.label == label)
    };
    // Every use of a root, by block and position; the terminator of a
    // block is at the position past its instructions.
    let mut uses = HashMap::<&str, Vec<(usize, usize)>>::new();
    let mut captured = HashSet::<&str>::new();
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let places = named_places(instruction);
            if matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Closure { .. } | Rvalue::StartTask { .. },
                    ..
                }
            ) {
                captured.extend(places.iter().map(|place| root(place)));
            }
            for place in places {
                let at = uses.entry(root(place)).or_default();
                if at.last() != Some(&(block_index, index)) {
                    at.push((block_index, index));
                }
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            uses.entry(root(place))
                .or_default()
                .push((block_index, block.instructions.len()));
        }
    }
    let uses_of = |name: &str| uses.get(name).map(Vec::as_slice).unwrap_or_default();
    let instruction_at =
        |(block, index): (usize, usize)| function.blocks[block].instructions.get(index);
    let at_label = |(block, index): (usize, usize)| (function.blocks[block].label.clone(), index);

    let mut found = Vec::new();
    for (split_block, block) in function.blocks.iter().enumerate() {
        for (split_at, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Assign {
                target: list,
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
            } = instruction
            else {
                continue;
            };
            let Some((kind, separator)) = walk_of(*op, args) else {
                continue;
            };
            let held = receiver.contains('.');
            if !local_is(list, &texts)
                || !(held || is_param(receiver) || local_is(receiver, &text))
                || captured.contains(root(receiver))
            {
                continue;
            }
            // The region: every position a path from the split reaches
            // before a release of the list.
            let mut region = HashSet::new();
            let mut work = vec![(split_block, split_at + 1)];
            let mut entered = HashSet::new();
            let mut whole = true;
            while let Some((at_block, from)) = work.pop() {
                if !entered.insert((at_block, from)) {
                    continue;
                }
                let instructions = &function.blocks[at_block].instructions;
                let mut ended = false;
                for (index, instruction) in instructions.iter().enumerate().skip(from) {
                    if (at_block, index) == (split_block, split_at) {
                        ended = true;
                        break;
                    }
                    region.insert((at_block, index));
                    if matches!(
                        instruction,
                        Instruction::Assign {
                            value: Rvalue::Try { .. },
                            ..
                        }
                    ) {
                        whole = false;
                    }
                    if matches!(instruction, Instruction::Release { place } if place == list) {
                        ended = true;
                        break;
                    }
                }
                if ended {
                    continue;
                }
                region.insert((at_block, instructions.len()));
                for label in super::elements::successors(&function.blocks[at_block].terminator) {
                    match block_of(label) {
                        Some(next) => work.push((next, 0)),
                        None => whole = false,
                    }
                }
            }
            if !whole {
                continue;
            }
            // The list: the split, one length read, one element read at
            // the index, and releases.
            let mut len = None;
            let mut fetch = None;
            let mut list_releases = Vec::new();
            let mut plain = true;
            for &at in uses_of(list) {
                if at == (split_block, split_at) {
                    continue;
                }
                match instruction_at(at).filter(|_| region.contains(&at)) {
                    Some(Instruction::Release { place }) if place == list => {
                        list_releases.push(at_label(at));
                    }
                    Some(Instruction::Assign {
                        target,
                        value:
                            Rvalue::Call {
                                callee:
                                    CallTarget::Runtime {
                                        op: RuntimeOp::List(ListOp::Len),
                                        receiver: Some(Operand::Place(source)),
                                        ..
                                    },
                                args,
                                ..
                            },
                    }) if source == list
                        && args.is_empty()
                        && len.is_none()
                        && at.0 == split_block
                        && !target.contains('.') =>
                    {
                        len = Some((at, target.clone()));
                    }
                    Some(Instruction::IndexInRange {
                        list: ranged,
                        index,
                    }) if ranged == list => match &fetch {
                        Some((read, _, counter))
                            if (at.0, at.1 + 1) == *read && counter == index => {}
                        None => match instruction_at((at.0, at.1 + 1)) {
                            Some(Instruction::Assign {
                                target,
                                value:
                                    Rvalue::Call {
                                        callee:
                                            CallTarget::Runtime {
                                                op: RuntimeOp::List(ListOp::IndexUnchecked),
                                                receiver: Some(Operand::Place(source)),
                                                ..
                                            },
                                        args,
                                        ..
                                    },
                            }) if source == list
                                && matches!(args.first(), Some(MirArg { value: Operand::Place(place), .. }) if place == index) =>
                            {
                                fetch = Some(((at.0, at.1 + 1), target.clone(), index.clone()));
                            }
                            _ => plain = false,
                        },
                        _ => plain = false,
                    },
                    Some(Instruction::Assign { .. })
                        if fetch.as_ref().is_some_and(|(read, _, _)| *read == at) => {}
                    _ => plain = false,
                }
            }
            let (Some((len_at, len_target)), Some((fetch_at, element, counter)), true) =
                (len, fetch, plain)
            else {
                continue;
            };
            // The fact and the read it covers come in that order, so a
            // read met first is no read of this shape.
            if !matches!(
                instruction_at((fetch_at.0, fetch_at.1.wrapping_sub(1))),
                Some(Instruction::IndexInRange { list: ranged, index }) if ranged == list && *index == counter
            ) || list_releases.is_empty()
            {
                continue;
            }
            // The index: zero in the block of the split, one more right
            // after the element read, and otherwise only compared.
            let mut zeroed = false;
            let mut stepped = false;
            let counted = !counter.contains('.')
                && !is_param(&counter)
                && uses_of(&counter)
                    .iter()
                    .all(|&at| match instruction_at(at) {
                        Some(Instruction::Assign {
                            target,
                            value: Rvalue::Use(Operand::Int(0)),
                        }) if *target == counter
                            && at.0 == split_block
                            && at.1 > split_at
                            && !zeroed =>
                        {
                            zeroed = true;
                            true
                        }
                        Some(Instruction::Assign {
                            target,
                            value:
                                Rvalue::Binary {
                                    op: BinaryOp::Add,
                                    left: Operand::Place(left),
                                    right: Operand::Int(1),
                                    ..
                                },
                        }) if *target == counter
                            && *left == counter
                            && at == (fetch_at.0, fetch_at.1 + 1)
                            && !stepped =>
                        {
                            stepped = true;
                            true
                        }
                        Some(Instruction::Assign {
                            target,
                            value:
                                Rvalue::Binary {
                                    left: Operand::Place(_) | Operand::Int(_),
                                    right: Operand::Place(_) | Operand::Int(_),
                                    ..
                                },
                        }) => root(target) != counter,
                        Some(Instruction::IndexInRange { .. }) => {
                            at == (fetch_at.0, fetch_at.1 - 1)
                        }
                        Some(Instruction::Assign { .. }) => at == fetch_at,
                        _ => false,
                    });
            if !(counted && zeroed && stepped) {
                continue;
            }
            // The element: read by instructions that only read a `str`,
            // and released, inside the region.
            if !local_is(&element, &text)
                || element == *receiver
                || captured.contains(element.as_str())
            {
                continue;
            }
            let read = uses_of(&element).iter().all(|&at| {
                if at == fetch_at {
                    return true;
                }
                if !region.contains(&at) {
                    return false;
                }
                match instruction_at(at) {
                    Some(Instruction::Release { place }) if *place == element => true,
                    Some(reader) => reads_part(reader, &element),
                    None => false,
                }
            });
            if !read {
                continue;
            }
            // The receiver: named in the region only by one release in
            // the block of the split. A part of a local is not released,
            // and the places its local borrows from, when it is a loan,
            // stay unnamed too.
            let mut receiver_release = None;
            let mut still = uses_of(root(receiver)).iter().all(|&at| {
                if !region.contains(&at) {
                    return true;
                }
                match instruction_at(at) {
                    Some(Instruction::Release { place })
                        if place == receiver
                            && !held
                            && at.0 == split_block
                            && receiver_release.is_none() =>
                    {
                        receiver_release = Some(at_label(at));
                        true
                    }
                    // The loop holds the part of a local it walks, so
                    // the local may go once the walk has started.
                    Some(Instruction::Release { place }) => {
                        held && place == root(receiver) && at.0 == split_block
                    }
                    _ => false,
                }
            });
            if held {
                for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
                    let lends = matches!(
                        instruction,
                        Instruction::BeginLoan { loan, .. }
                        | Instruction::BeginElementLoan { loan, .. }
                        | Instruction::BeginReturnedLoan { loan, .. }
                        | Instruction::Reborrow { loan, .. }
                        | Instruction::ReborrowElement { loan, .. } if root(loan) == root(receiver)
                    );
                    if lends {
                        still &= named_places(instruction).iter().all(|place| {
                            uses_of(root(place)).iter().all(|at| !region.contains(at))
                        });
                    }
                }
            }
            if !still {
                continue;
            }
            found.push(SplitLoop {
                list: list.clone(),
                receiver: receiver.clone(),
                held,
                separator,
                kind,
                split: at_label((split_block, split_at)),
                len: at_label(len_at),
                len_target,
                fetch: at_label(fetch_at),
                element,
                list_releases,
                receiver_release,
            });
        }
    }
    // A local that two loops would fill, or a position two loops would
    // compile, is left to the list.
    let mut names = HashMap::<&str, usize>::new();
    for found in &found {
        for name in [&found.list, &found.element, &found.len_target] {
            *names.entry(name.as_str()).or_default() += 1;
        }
    }
    let shared = found
        .iter()
        .filter(|found| {
            [&found.list, &found.element, &found.len_target]
                .iter()
                .any(|name| names[name.as_str()] > 1)
        })
        .map(|found| found.list.clone())
        .collect::<HashSet<_>>();
    found.retain(|found| !shared.contains(&found.list));
    found
}

impl FunctionCompiler<'_> {
    /// Plans the loops of `function` that walk a text with no list (see
    /// [`split_loops`]), leaving out each whose receiver this body cannot
    /// hold still for the region: it must be a parameter, a local that
    /// owns its box, or the element of another such loop, whose region
    /// then holds this one. A list or an element that another way of
    /// compiling the function claims keeps its list too.
    pub(super) fn plan_split_loops(&mut self, function: &MirFunction) {
        let mut loops = split_loops(function);
        loops.retain(|found| {
            let lists = std::slice::from_ref(&found.list);
            let at = |at: &(String, usize)| self.split_part_steps.contains_key(at);
            !self.claims_split_local(&found.list, lists)
                && !self.claims_split_local(&found.element, lists)
                && !self.split_part_locals.contains(&found.receiver)
                && !at(&found.split)
                && !at(&found.len)
                && !at(&found.fetch)
                && !found.receiver_release.as_ref().is_some_and(at)
                && !found.list_releases.iter().any(at)
        });
        loop {
            let elements = loops
                .iter()
                .map(|found| found.element.clone())
                .collect::<HashSet<_>>();
            let before = loops.len();
            loops.retain(|found| {
                let receiver = found.receiver.as_str();
                found.held
                    || elements.contains(receiver)
                    || function.params.iter().any(|param| param.name == receiver)
                    || (!self.str_slots.contains_key(receiver) && self.owns_counted_str(receiver))
            });
            if loops.len() == before {
                break;
            }
        }
        for found in loops {
            let declare = |this: &mut Self| {
                let variable = Var(this.next_variable_index as u32);
                this.next_variable_index += 1;
                this.builder.declare_var(variable, types::I64);
                let zero = this.builder.iconst(types::I64, 0);
                this.builder.def_var(variable, zero);
                variable
            };
            let (ptr, len, cursor) = (declare(self), declare(self), declare(self));
            let kept = found.held.then(|| declare(self));
            let walk = Walk {
                separator: found.separator.clone(),
                kind: found.kind,
                ptr,
                len,
                cursor,
            };
            self.split_part_steps.insert(
                found.split.clone(),
                Step::LoopSplit {
                    receiver: found.receiver.clone(),
                    list: found.list.clone(),
                    walk: walk.clone(),
                    kept,
                },
            );
            self.split_part_steps.insert(
                found.len.clone(),
                Step::LoopLen {
                    target: found.len_target.clone(),
                    walk: walk.clone(),
                },
            );
            self.split_part_steps.insert(
                found.fetch.clone(),
                Step::LoopFetch {
                    element: found.element.clone(),
                    walk,
                },
            );
            if let Some(release) = &found.receiver_release {
                self.split_part_steps.insert(release.clone(), Step::Nothing);
            }
            for release in &found.list_releases {
                self.split_part_steps.insert(
                    release.clone(),
                    Step::LoopRelease {
                        receiver: found
                            .receiver_release
                            .as_ref()
                            .map(|_| found.receiver.clone()),
                        kept,
                    },
                );
            }
            // The loop reads no view of the list it does not make.
            self.list_snapshots
                .retain(|snapshot| snapshot.region.list != found.list);
            // The element may already be an element slot of the list; it
            // reads the text now.
            self.str_element_lists.remove(&found.element);
            self.split_part_locals.insert(found.element.clone());
            let slot = str_slots::StrSlot::local(declare(self), declare(self));
            self.str_slots.insert(found.element.clone(), slot);
        }
    }

    /// Keeps the words of the receiver of a loop that walks it, and
    /// starts the walk. A receiver that is a part of a local is read into
    /// `kept` as a reference of the loop's own, a copy where the read
    /// makes one, which the releases of the list give up: the text then
    /// stays where it is whatever becomes of the local.
    pub(super) fn compile_loop_split(
        &mut self,
        receiver: &str,
        list: &str,
        walk: &Walk,
        kept: Option<Var>,
    ) -> std::result::Result<(), String> {
        let words = match kept {
            Some(kept) => {
                let loaded = self.load_place(receiver)?;
                let loaded = self.ensure_opaque(loaded)?;
                let held = self.transfer_opaque_arg(&loaded);
                self.builder.def_var(kept, held);
                self.str_box_words(held)
            }
            None => self.split_receiver_words(receiver)?,
        };
        self.builder.def_var(walk.ptr, words.0);
        self.builder.def_var(walk.len, words.1);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(walk.cursor, zero);
        // The list is never made: its word stays empty.
        for variable in self.local_vars(list)? {
            self.builder.def_var(variable, zero);
        }
        Ok(())
    }

    /// The text and the separator of `walk`, as the words the walk's
    /// entry points take first.
    fn walk_words(&mut self, walk: &Walk) -> std::result::Result<[Val; 5], String> {
        let (separator, separator_len) = self.string_constant(walk.separator.as_bytes())?;
        let kind = self.builder.iconst(types::I64, walk.kind);
        Ok([
            self.builder.use_var(walk.ptr),
            self.builder.use_var(walk.len),
            separator,
            separator_len,
            kind,
        ])
    }

    /// Counts the parts of `walk` into `target`, the length the list
    /// would have.
    pub(super) fn compile_loop_len(
        &mut self,
        target: &str,
        walk: &Walk,
    ) -> std::result::Result<(), String> {
        let words = self.walk_words(walk)?;
        let count = self
            .builder
            .call(self.runtime[RuntimeFn::StrPartsCountView], &words)
            .results[0];
        self.store_place(
            target,
            ValueRef {
                values: vec![count],
                ty: DirectType::Scalar(ScalarKind::Int64),
            },
        )
    }

    /// Finds the next part of `walk` and gives it to `element` as words.
    pub(super) fn compile_loop_fetch(
        &mut self,
        element: &str,
        walk: &Walk,
    ) -> std::result::Result<(), String> {
        let mut words = self.walk_words(walk)?.to_vec();
        let slot = self.builder.create_sized_stack_slot(16, 3);
        let cursor_at = self.builder.stack_addr(types::I64, slot, 0);
        let len_at = self.builder.stack_addr(types::I64, slot, 8);
        let cursor = self.builder.use_var(walk.cursor);
        self.builder
            .store(MemFlags::trusted(), cursor, cursor_at, 0);
        words.extend([cursor_at, len_at]);
        let part = self
            .builder
            .call(self.runtime[RuntimeFn::StrPartsNextView], &words)
            .results[0];
        let cursor = self
            .builder
            .load(types::I64, MemFlags::trusted(), cursor_at, 0);
        self.builder.def_var(walk.cursor, cursor);
        let part_len = self
            .builder
            .load(types::I64, MemFlags::trusted(), len_at, 0);
        self.define_str_slot(element, part, part_len)
    }
}

impl FunctionCompiler<'_> {
    /// Gives up the reference a loop held to its receiver in `kept`.
    pub(super) fn release_loop_receiver(&mut self, kept: Var) {
        let held = self.builder.use_var(kept);
        self.release_opaque_handle(held);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(kept, zero);
    }
}

/// The words a loop keeps for its walk: the text, and the place the walk
/// has reached.
#[derive(Clone, Debug)]
pub(super) struct Walk {
    pub(super) separator: String,
    pub(super) kind: i64,
    pub(super) ptr: Var,
    pub(super) len: Var,
    pub(super) cursor: Var,
}

#[cfg(test)]
#[path = "native_codegen_split_loops_tests.rs"]
mod tests;
