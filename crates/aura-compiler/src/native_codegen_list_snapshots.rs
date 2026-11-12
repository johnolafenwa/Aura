//! Finds the loops that only measure, index, or loan elements of a local
//! list, whose list view native code reads once, when the loop is entered.
//!
//! Every in-place list operation reads the list view first: its access
//! code, element address, and length (see `DirectListLayout` in the
//! runtime). Inside a loop, LLVM cannot keep those words in registers,
//! because each operation's slow path is a runtime call that might change
//! them, so every iteration loads and checks them again. In a loop whose
//! only instructions that name the list are these operations, the words do
//! not change while the loop runs, as long as the view is one of the
//! forms below when the loop is entered:
//!
//! - A read (`len`, an index read of a scalar or plain record element)
//!   only reads the list. Its runtime call may take in elements generated
//!   code appended, which sets the list's room, but leaves the view's
//!   access code, element address, and length as they were.
//! - Any other operation here (an element loan, an index read of a box)
//!   may write the list through the runtime, which refreshes the view. A
//!   list whose storage nothing else shares keeps its storage, form, and
//!   length through such a write, because none of these operations adds or
//!   removes elements. Room tells generated code the storage is unshared,
//!   so a loop with such operations reads the view once only when the list
//!   has room as the loop is entered.
//! - A list of values (`DirectListAccess::Values`) may change its form
//!   when the runtime promotes an element or builds a `str` view, and a
//!   shared list has no form generated code reads. Neither reads its view
//!   once.
//!
//! Nothing outside these instructions reaches the list while the loop
//! runs: a callee reaches a local list only when it is passed in, and a
//! list a closure, a task, or a loan of the whole list can reach is never
//! chosen. Native code therefore copies the three words into variables on
//! every edge that enters the loop, with a flag that says whether the view
//! qualifies, and each operation reads a word from its variable when the
//! flag is set and from the list otherwise. The variables are defined only
//! outside the loop, so LLVM can split the loop on the flag and drop every
//! view check from the copy that runs when it holds.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::mir::ownership::instruction_accesses;
use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MirFunction,
    Operand, Rvalue,
};
use crate::runtime_ops::{ListOp, RuntimeOp};

/// One loop that reads the view of `list` once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ListSnapshotRegion {
    /// The local list.
    pub(super) list: String,
    /// The blocks of the loop.
    pub(super) blocks: HashSet<String>,
    /// Each edge from a block outside the loop to a block in it.
    pub(super) entries: Vec<(String, String)>,
    /// Whether an operation in the loop may write the list through the
    /// runtime, so the view is read once only when the list has room.
    pub(super) writes: bool,
}

fn root(place: &str) -> &str {
    place.split(['.', '[']).next().unwrap_or(place)
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

/// How an instruction that names `list` uses it inside a loop that reads
/// the view once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ViewUse {
    /// Reads the list only.
    Reads,
    /// May write an element of the list through the runtime.
    Writes,
}

/// The use `instruction` makes of `list` when it is one of the operations
/// a loop may hold, or `None`. `pure_index` says whether an index read of
/// `list` only reads it.
fn view_use(instruction: &Instruction, list: &str, pure_index: bool) -> Option<ViewUse> {
    match instruction {
        Instruction::IndexInRange { list: named, index } if named == list && root(index) != list => {
            Some(ViewUse::Reads)
        }
        Instruction::BeginElementLoan {
            loan,
            source,
            selector,
            ..
        } if source == list
            && root(loan) != list
            && !matches!(selector, Operand::Place(place) | Operand::MovePlace(place) if root(place) == list) =>
        {
            Some(ViewUse::Writes)
        }
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
        } if receiver == list
            && root(target) != list
            && args.iter().all(|arg| {
                !matches!(&arg.value, Operand::Place(place) | Operand::MovePlace(place) if root(place) == list)
                    && arg.writeback_place.as_deref().is_none_or(|place| root(place) != list)
            }) =>
        {
            match op {
                ListOp::Len | ListOp::IndexScalar(_) | ListOp::IndexScalarUnchecked(_) => {
                    Some(ViewUse::Reads)
                }
                // A store into unshared storage of the element's own kind
                // keeps the storage, its form, and its length.
                ListOp::SetIndexScalar(_) => Some(ViewUse::Writes),
                ListOp::Index | ListOp::IndexUnchecked if pure_index => Some(ViewUse::Reads),
                ListOp::Index | ListOp::IndexUnchecked => Some(ViewUse::Writes),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether `instruction` lets something other than this function's own
/// instructions on `list` reach it: a closure or a task that captures it,
/// a loan of the whole list or of a place in it, or a cleanup action.
fn reaches_list(instruction: &Instruction, list: &str) -> bool {
    match instruction {
        Instruction::Assign {
            value: Rvalue::Closure { .. } | Rvalue::StartTask { .. },
            ..
        } => mentioned_roots(instruction).contains(list),
        Instruction::BeginLoan { source, .. }
        | Instruction::BeginReturnedLoan { origin: source, .. }
        | Instruction::Reborrow { parent: source, .. }
        | Instruction::ReborrowElement { parent: source, .. }
        | Instruction::PushCleanup { place: source, .. } => root(source) == list,
        _ => false,
    }
}

/// The loops of `function` that read the view of a list once. `candidate`
/// says which locals hold a list native code may read this way, and
/// `pure_index` whether an index read of one only reads it.
pub(super) fn list_snapshot_regions(
    function: &MirFunction,
    candidate: impl Fn(&str) -> bool,
    pure_index: impl Fn(&str) -> bool,
) -> Vec<ListSnapshotRegion> {
    let mut lists = Vec::new();
    let mut seen = HashSet::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Some(list) = match instruction {
                Instruction::IndexInRange { list, .. } => Some(list.as_str()),
                Instruction::BeginElementLoan { source, .. } => Some(source.as_str()),
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee:
                                CallTarget::Runtime {
                                    op: RuntimeOp::List(_),
                                    receiver: Some(Operand::Place(receiver)),
                                    ..
                                },
                            ..
                        },
                    ..
                } => Some(receiver.as_str()),
                _ => None,
            } {
                if !list.contains(['.', '[']) && candidate(list) && seen.insert(list.to_string()) {
                    lists.push(list.to_string());
                }
            }
        }
    }
    let successors = function
        .blocks
        .iter()
        .map(|block| {
            (
                block.label.as_str(),
                super::direct_terminator_successors(&block.terminator),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut regions = Vec::new();
    for list in lists {
        let pure = pure_index(&list);
        let instructions = || function.blocks.iter().flat_map(|block| &block.instructions);
        if instructions().any(|instruction| reaches_list(instruction, &list)) {
            continue;
        }
        // A loan of an element begun anywhere may write the list later.
        let loans_elements = instructions().any(|instruction| {
            matches!(instruction, Instruction::BeginElementLoan { source, .. } if root(source) == list)
        });
        // The blocks whose every mention of the list is an operation that
        // keeps its view, with whether one may write it and whether one
        // uses it at all.
        let mut clean = HashMap::<&str, (bool, bool)>::new();
        'blocks: for block in &function.blocks {
            let mut writes = false;
            let mut uses = false;
            for instruction in &block.instructions {
                if !mentioned_roots(instruction).contains(list.as_str()) {
                    continue;
                }
                match view_use(instruction, &list, pure) {
                    Some(use_) => {
                        uses = true;
                        writes |= use_ == ViewUse::Writes;
                    }
                    None => continue 'blocks,
                }
            }
            if terminator_place_mentions(&block.terminator)
                .into_iter()
                .any(|place| root(place) == list)
            {
                continue;
            }
            clean.insert(block.label.as_str(), (writes, uses));
        }
        for component in strongly_connected(&clean, &successors) {
            let cyclic = component.len() > 1
                || component.iter().any(|label| {
                    successors
                        .get(label)
                        .is_some_and(|next| next.contains(label))
                });
            if !cyclic || component.contains(function.entry.as_str()) {
                continue;
            }
            if !component.iter().any(|label| clean[label].1) {
                continue;
            }
            let writes = loans_elements || component.iter().any(|label| clean[label].0);
            let mut entries = Vec::new();
            for block in &function.blocks {
                if component.contains(block.label.as_str()) {
                    continue;
                }
                for next in &successors[block.label.as_str()] {
                    if component.contains(next) {
                        let edge = (block.label.clone(), next.to_string());
                        if !entries.contains(&edge) {
                            entries.push(edge);
                        }
                    }
                }
            }
            if entries.is_empty() {
                continue;
            }
            regions.push(ListSnapshotRegion {
                list: list.clone(),
                blocks: component.iter().map(|label| label.to_string()).collect(),
                entries,
                writes,
            });
        }
    }
    regions
}

/// The strongly connected components of the graph of `nodes`, with the
/// edges of `successors` between them, in Tarjan's order.
fn strongly_connected<'f, T>(
    nodes: &HashMap<&'f str, T>,
    successors: &HashMap<&'f str, Vec<&'f str>>,
) -> Vec<HashSet<&'f str>> {
    struct State<'f> {
        index: HashMap<&'f str, usize>,
        low: HashMap<&'f str, usize>,
        on_stack: HashSet<&'f str>,
        stack: Vec<&'f str>,
        next: usize,
        components: Vec<HashSet<&'f str>>,
    }
    let mut state = State {
        index: HashMap::new(),
        low: HashMap::new(),
        on_stack: HashSet::new(),
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    let edges = |node: &str| -> Vec<&'f str> {
        successors
            .get(node)
            .map(|next| {
                next.iter()
                    .copied()
                    .filter(|label| nodes.contains_key(label))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut order = nodes.keys().copied().collect::<Vec<_>>();
    order.sort_unstable();
    for start in order {
        if state.index.contains_key(start) {
            continue;
        }
        // An explicit stack of (node, next edge position), so deep CFGs
        // cannot overflow the compiler's stack.
        let mut work = vec![(start, 0usize)];
        state.index.insert(start, state.next);
        state.low.insert(start, state.next);
        state.next += 1;
        state.stack.push(start);
        state.on_stack.insert(start);
        while let Some((node, position)) = work.pop() {
            let next = edges(node);
            if let Some(&child) = next.get(position) {
                work.push((node, position + 1));
                if !state.index.contains_key(child) {
                    state.index.insert(child, state.next);
                    state.low.insert(child, state.next);
                    state.next += 1;
                    state.stack.push(child);
                    state.on_stack.insert(child);
                    work.push((child, 0));
                } else if state.on_stack.contains(child) {
                    let low = state.low[node].min(state.index[child]);
                    state.low.insert(node, low);
                }
                continue;
            }
            if state.low[node] == state.index[node] {
                let mut component = HashSet::new();
                while let Some(member) = state.stack.pop() {
                    state.on_stack.remove(member);
                    component.insert(member);
                    if member == node {
                        break;
                    }
                }
                state.components.push(component);
            }
            if let Some(&(parent, _)) = work.last() {
                let low = state.low[parent].min(state.low[node]);
                state.low.insert(parent, low);
            }
        }
    }
    state.components
}

/// A word of a list view native code may read once per loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ViewWord {
    Access,
    Len,
    Elements,
    /// How many elements the storage has room for, which is not 0 only
    /// while no other list shares it.
    Room,
}

/// A loop that reads the view of a list once, with the variables that hold
/// the view's words inside it.
#[derive(Clone, Debug)]
pub(super) struct ListSnapshot {
    pub(super) region: ListSnapshotRegion,
    /// 1 while the words below are the list's view, 0 otherwise.
    pub(super) ok: Var,
    pub(super) access: Var,
    pub(super) len: Var,
    pub(super) elements: Var,
    /// The blocks native code built for the loop's MIR blocks.
    pub(super) ir_blocks: Vec<Blk>,
    /// The selects of the loop's view reads, with whether each reads the
    /// access code.
    pub(super) selects: Vec<(Val, bool)>,
    /// Each block that enters the loop, with its flag and the access code
    /// it read.
    pub(super) entries: Vec<(Blk, Val, Val)>,
    /// The access codes the loop's operations check the view for.
    pub(super) expected: Vec<i64>,
    /// The element accesses that stay in range while the guards hold
    /// (see `native_codegen_range_loops`).
    pub(super) range: Option<super::range_loops::RangeProof>,
    /// The call slot of the mutable parameter that holds the list, whose
    /// write-through sink must be absent for the loop's copy to run.
    pub(super) sink: Option<usize>,
}

impl FunctionCompiler<'_> {
    /// The loops that block `label` belongs to, as indexes into
    /// `list_snapshots`, to read in place of the views of their lists.
    pub(super) fn enter_snapshot_block(&mut self, label: &str) {
        self.snapshot_vals.clear();
        self.current_snapshot_label = label.to_string();
    }

    /// The snapshot that holds the view of root `name` in the block being
    /// compiled.
    pub(super) fn snapshot_of_root(&self, name: &str) -> Option<usize> {
        self.list_snapshots.iter().position(|snapshot| {
            snapshot.region.list == name
                && snapshot
                    .region
                    .blocks
                    .contains(&self.current_snapshot_label)
        })
    }

    /// `handle`, the box root `name` holds, as a value of its own that the
    /// view reads of the block's loop know, when the block is in one.
    pub(super) fn snapshot_handle(&mut self, name: &str, handle: Val) -> Val {
        let Some(index) = self.snapshot_of_root(name) else {
            return handle;
        };
        // A value of its own, so another local that holds the same box
        // still reads its view from memory.
        let own = self.builder.iadd_imm(handle, 0);
        self.snapshot_vals.insert(own, index);
        own
    }

    /// The block a jump from the block being compiled to `target` (MIR
    /// block `label`) goes to: `target`, or the block that reads the views
    /// of the loops the jump enters.
    pub(super) fn snapshot_edge(&mut self, label: &str, target: Blk) -> Blk {
        let entering = self
            .list_snapshots
            .iter()
            .enumerate()
            .filter(|(_, snapshot)| {
                snapshot
                    .region
                    .entries
                    .iter()
                    .any(|(from, to)| *from == self.current_snapshot_label && to == label)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if entering.is_empty() {
            return target;
        }
        if let Some((pad, _, _)) = self
            .pending_snapshot_pads
            .iter()
            .find(|(_, existing, _)| *existing == target)
        {
            return *pad;
        }
        let pad = self.builder.create_block();
        self.pending_snapshot_pads.push((pad, target, entering));
        pad
    }

    /// Fills the blocks the terminator just compiled jumps through to
    /// enter loops: each reads the views of its loops' lists and jumps on.
    pub(super) fn fill_snapshot_pads(&mut self) -> std::result::Result<(), String> {
        for (pad, target, entering) in std::mem::take(&mut self.pending_snapshot_pads) {
            self.builder.switch_to_block(pad);
            self.builder.seal_block(pad);
            let mut oks = Vec::new();
            for index in &entering {
                oks.push((*index, self.emit_list_snapshot(*index)?));
            }
            // A loop that reads several lists' views once runs its copy
            // while every view qualifies. The jump on is in the block the
            // reads end in.
            let pad = self.builder.current_block().unwrap_or(pad);
            for (index, (_, access)) in &oks {
                let mut flag = None;
                for (other, (ok, _)) in &oks {
                    if self.list_snapshots[*other].region.blocks
                        == self.list_snapshots[*index].region.blocks
                    {
                        flag = Some(match flag {
                            None => *ok,
                            Some(flag) => self.builder.band(flag, *ok),
                        });
                    }
                }
                let flag = flag.expect("a loop's own flag is among them");
                self.list_snapshots[*index]
                    .entries
                    .push((pad, flag, *access));
            }
            self.builder.jump(target, &[]);
        }
        Ok(())
    }

    /// Copies the view of snapshot `index`'s list into its variables, with
    /// whether the view qualifies (see the module documentation).
    fn emit_list_snapshot(&mut self, index: usize) -> std::result::Result<(Val, Val), String> {
        use aura_runtime::native_runtime::{
            DirectListAccess, FIRST_CLASS_ACCESS, FIRST_PLAIN_ACCESS,
        };
        let snapshot = self.list_snapshots[index].clone();
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let vars = self.local_vars(&snapshot.region.list)?;
        let [var] = vars.as_slice() else {
            return Err(format!(
                "direct backend read the view of `{}`, which is not one box",
                snapshot.region.list
            ));
        };
        let handle = self.builder.use_var(*var);
        let read_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        for _ in 0..4 {
            self.builder.append_block_param(join_block, types::I64);
        }
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.brif(
            handle,
            read_block,
            &[],
            join_block,
            &[zero, zero, zero, zero],
        );

        self.builder.switch_to_block(read_block);
        self.builder.seal_block(read_block);
        let access = self.builder.uload32(flags, handle, layout.access_offset);
        let len = self
            .builder
            .load(types::I64, flags, handle, layout.len_offset);
        let elements = self
            .builder
            .load(types::I64, flags, handle, layout.elements_offset);
        // A list of scalars other than `str` views, of boxes, or of plain
        // records.
        let scalar_form = self
            .builder
            .iadd_imm(access, -(DirectListAccess::Int64 as i64));
        let scalar = self.builder.icmp_imm(
            IntCC::UnsignedLessThan,
            scalar_form,
            DirectListAccess::Float32 as i64 - DirectListAccess::Int64 as i64 + 1,
        );
        let not_str = self
            .builder
            .icmp_imm(IntCC::NotEqual, access, DirectListAccess::Str as i64);
        let scalar = self.builder.band(scalar, not_str);
        let boxes = self
            .builder
            .icmp_imm(IntCC::Equal, access, DirectListAccess::Boxes as i64);
        let typed_form = self
            .builder
            .iadd_imm(access, -i64::from(FIRST_PLAIN_ACCESS));
        let typed = self.builder.icmp_imm(
            IntCC::UnsignedLessThan,
            typed_form,
            i64::from(FIRST_CLASS_ACCESS - FIRST_PLAIN_ACCESS),
        );
        let form = self.builder.bor(scalar, boxes);
        let mut qualifies = self.builder.bor(form, typed);
        if snapshot.region.writes {
            let room = self.builder.uload32(flags, handle, layout.capacity_offset);
            let has_room = self.builder.icmp_imm(IntCC::NotEqual, room, 0);
            qualifies = self.builder.band(qualifies, has_room);
        }
        if let Some(slot) = snapshot.sink {
            // A caller that asked for each write to be handed back takes
            // a reference to the list with it, so the storage is shared.
            let sink = self.current_mutable_sink(slot);
            let direct = self.builder.icmp_imm(IntCC::Equal, sink, 0);
            qualifies = self.builder.band(qualifies, direct);
        }
        if let Some(proof) = &snapshot.range {
            let (code, _) = super::scalar_list_slot(proof.element);
            let exact = self.builder.icmp_imm(IntCC::Equal, access, code as i64);
            qualifies = self.builder.band(qualifies, exact);
            for guard in &proof.guards {
                use super::range_loops::{RangeBound, RangeGuard};
                let holds = match guard {
                    RangeGuard::AtLeast { place, least } => {
                        let value = self.range_word(place)?;
                        self.builder
                            .icmp_imm(IntCC::SignedGreaterThanOrEqual, value, *least)
                    }
                    RangeGuard::Within { bound, distance } => {
                        let bound = match bound {
                            RangeBound::Place(place) => self.range_word(place)?,
                            RangeBound::Int(value) => self.builder.iconst(types::I64, *value),
                        };
                        // A length is far below the range of `int64`, so
                        // the difference is exact.
                        let limit = self.builder.iadd_imm(len, -*distance);
                        self.builder
                            .icmp(IntCC::SignedLessThanOrEqual, bound, limit)
                    }
                };
                qualifies = self.builder.band(qualifies, holds);
            }
        }
        let ok = self.builder.uextend(types::I64, qualifies);
        self.builder.jump(join_block, &[ok, access, len, elements]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let params = self.builder.block_params(join_block).to_vec();
        for (variable, value) in [
            snapshot.ok,
            snapshot.access,
            snapshot.len,
            snapshot.elements,
        ]
        .into_iter()
        .zip(params.iter().copied())
        {
            self.builder.def_var(variable, value);
        }
        Ok((params[0], params[1]))
    }

    /// The value of the `int64` local `place`, which a range guard reads.
    fn range_word(&mut self, place: &str) -> std::result::Result<Val, String> {
        let vars = self.local_vars(place)?;
        let [var] = vars.as_slice() else {
            return Err(format!(
                "direct backend guarded a range on `{place}`, which is not one word"
            ));
        };
        Ok(self.builder.use_var(*var))
    }

    /// The loop whose copy runs the access being compiled on `list` with
    /// no bounds check, as an index into `list_snapshots`.
    pub(super) fn range_covered(&self, list: Val) -> Option<usize> {
        let index = *self.snapshot_vals.get(&list)?;
        let proof = self.list_snapshots[index].range.as_ref()?;
        proof
            .covered
            .contains(&(
                self.current_snapshot_label.clone(),
                self.current_snapshot_instruction,
            ))
            .then_some(index)
    }

    /// The loop whose copy computes the checked sum being compiled with no
    /// overflow check, as an index into `list_snapshots`.
    pub(super) fn range_exact(&self) -> Option<usize> {
        let position = (
            self.current_snapshot_label.clone(),
            self.current_snapshot_instruction,
        );
        self.list_snapshots.iter().position(|snapshot| {
            snapshot
                .range
                .as_ref()
                .is_some_and(|proof| proof.exact.contains(&position))
        })
    }

    /// `kept` in the copy of loop `index` that runs while its view
    /// qualifies, and `otherwise` in the loop as written.
    pub(super) fn keep_in_copy(&mut self, index: usize, kept: Val, otherwise: Val) -> Val {
        let ok = self.builder.use_var(self.list_snapshots[index].ok);
        let holds = self.builder.icmp_imm(IntCC::NotEqual, ok, 0);
        let select = self.builder.select(holds, kept, otherwise);
        self.list_snapshots[index].selects.push((select, false));
        select
    }

    /// Notes that an operation checks the view of `list` for `access`.
    pub(super) fn expect_list_access(&mut self, list: Val, access: i64) {
        if let Some(&index) = self.snapshot_vals.get(&list) {
            self.list_snapshots[index].expected.push(access);
        }
    }

    /// Reads `word` of the view of `list`: from the variable its loop keeps
    /// it in while that holds the view, and from the list otherwise.
    pub(super) fn list_view_word(&mut self, list: Val, word: ViewWord) -> Val {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let read = match word {
            ViewWord::Access => self.builder.uload32(flags, list, layout.access_offset),
            ViewWord::Len => self
                .builder
                .load(types::I64, flags, list, layout.len_offset),
            ViewWord::Elements => {
                self.builder
                    .load(types::I64, flags, list, layout.elements_offset)
            }
            ViewWord::Room => self.builder.uload32(flags, list, layout.capacity_offset),
        };
        let Some(&index) = self.snapshot_vals.get(&list) else {
            return read;
        };
        let snapshot = &self.list_snapshots[index];
        let ok = snapshot.ok;
        let kept = match word {
            ViewWord::Access => self.builder.use_var(snapshot.access),
            ViewWord::Len => self.builder.use_var(snapshot.len),
            ViewWord::Elements => self.builder.use_var(snapshot.elements),
            // A loop that may write the list runs its copy only while the
            // storage is unshared, which it stays (see the module
            // documentation), so every element has room there.
            ViewWord::Room if snapshot.region.writes => {
                self.builder.iconst(types::I64, i64::from(u32::MAX))
            }
            ViewWord::Room => return read,
        };
        let ok = self.builder.use_var(ok);
        let holds = self.builder.icmp_imm(IntCC::NotEqual, ok, 0);
        let select = self.builder.select(holds, kept, read);
        self.list_snapshots[index]
            .selects
            .push((select, word == ViewWord::Access));
        select
    }

    /// Records that the blocks from `first` up to the block count, and
    /// `own`, were built for MIR block `label`.
    pub(super) fn note_snapshot_blocks(&mut self, label: &str, own: Blk, first: u32) {
        let last = self.builder.block_count();
        for snapshot in &mut self.list_snapshots {
            if snapshot.region.blocks.contains(label) {
                snapshot.ir_blocks.push(own);
                snapshot.ir_blocks.extend((first..last).map(Blk));
            }
        }
    }

    /// Gives each loop that reads a view once its copy for while the view
    /// qualifies. A loop that shares blocks with one already copied keeps
    /// reading its views from memory.
    pub(super) fn version_snapshot_loops(&mut self) {
        let mut copied = HashSet::<Blk>::new();
        let mut done = vec![false; self.list_snapshots.len()];
        for index in 0..self.list_snapshots.len() {
            if done[index] {
                continue;
            }
            // The loops with the same blocks, which one copy serves.
            let group = (index..self.list_snapshots.len())
                .filter(|other| {
                    self.list_snapshots[*other].region.blocks
                        == self.list_snapshots[index].region.blocks
                })
                .collect::<Vec<_>>();
            // A loop over one list whose operations all check for one
            // access code runs its copy only for that code, which the copy
            // then knows. A loop with a range proof checked the code where
            // it was entered.
            let code_of = |snapshot: &ListSnapshot| {
                snapshot
                    .range
                    .as_ref()
                    .map(|proof| super::scalar_list_slot(proof.element).0 as i64)
            };
            let expected = match group.as_slice() {
                [only] if code_of(&self.list_snapshots[*only]).is_none() => {
                    let codes = &self.list_snapshots[*only].expected;
                    codes
                        .first()
                        .copied()
                        .filter(|first| codes.iter().all(|code| code == first))
                }
                _ => None,
            };
            let mut blocks = Vec::new();
            let mut selects = Vec::new();
            for member in &group {
                done[*member] = true;
                let known = code_of(&self.list_snapshots[*member]).or(expected);
                blocks.extend(self.list_snapshots[*member].ir_blocks.iter().copied());
                selects.extend(
                    self.list_snapshots[*member]
                        .selects
                        .iter()
                        .map(|(select, access)| (*select, known.filter(|_| *access))),
                );
            }
            let overlaps = blocks.iter().any(|block| copied.contains(block));
            let entries = if overlaps {
                Vec::new()
            } else {
                self.list_snapshots[index]
                    .entries
                    .iter()
                    .map(|(pad, flag, access)| (*pad, *flag, expected.map(|code| (*access, code))))
                    .collect()
            };
            if self.builder.version_blocks(&blocks, &selects, &entries) {
                copied.extend(blocks);
            }
        }
    }
}

#[cfg(test)]
#[path = "native_codegen_list_snapshots_tests.rs"]
mod tests;
