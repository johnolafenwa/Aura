//! Slow paths that continue in a copy of the body instead of rejoining it.
//!
//! Generated code checks, before each in-place operation, that the value
//! is in the form the operation reads in place, and calls the runtime when
//! it is not. The runtime's way is cold, and it rejoins the fast way after
//! the call. The call may change any memory, so after the join LLVM loads
//! and checks again everything the fast way had in registers: the layout
//! of the instance whose field is read, its block, the field's box, the
//! list view.
//!
//! [`FunctionIr::continue_slow_paths`] gives a body a second copy. The
//! function enters the copy, and each edge from a cold block of the copy
//! to a warm block goes to that block in the first body instead, which is
//! the body as it was built: it runs on from there exactly as the body
//! would have. So the copy's warm blocks are reached only by warm ways,
//! and what one operation's checks found still holds at the next. A back
//! edge of a loop in the first body goes back to the copy, so a loop whose
//! iteration took a slow way runs its next iteration fast again.
//!
//! Only a slow way after which warm code reads again a word that the way
//! to it read leaves the copy: elsewhere the second body would only add
//! code. A body whose copy would add too much code keeps one body, and so
//! does one that calls itself, unless a slow way leaves the copy in a loop.
//!
//! Every block of the copy computes what its block computes, from the
//! copy's values, and the variables are the same memory in both, so a
//! move between the two at any edge is exact as long as each value is
//! defined on every way to each of its uses. An edge that would break
//! that keeps its target in its own body.

use std::collections::{HashMap, HashSet};

use super::{map_op, op_operands, op_targets, BinaryOp, BlockIr, FunctionIr, InstIr, Op, Val, Var};
use crate::native_backend::{AliasClass, Blk, FuncHandle, MemoryAccess};

/// A body of more recorded instructions gets no copy.
const MOST_INSTRUCTIONS: usize = 24_000;

/// The most blocks the search for a word read again looks at, before a
/// slow way and after it.
const READ_BLOCKS: usize = 64;

/// A copy that would add more recorded instructions to the blocks the
/// function reaches is not made, which keeps binaries lean: the largest
/// bodies are mostly code that runs once.
const MOST_GROWTH: usize = 8192;

impl FunctionIr {
    /// Gives the body a copy that the function runs, whose slow paths
    /// continue in the body as it was (see the module documentation).
    /// Returns whether the copy was made: a body with no slow path that
    /// rejoins a warm block which reads again, or one too large, stays as
    /// it is. So does a `recursive` body, one that calls itself, unless a
    /// slow way leaves the copy in a loop: elsewhere each of its calls
    /// ends what the copy's checks found, and the larger body only costs
    /// the recursion registers.
    pub(super) fn continue_slow_paths(
        &mut self,
        recursive: bool,
        imported: &dyn Fn(FuncHandle) -> bool,
    ) -> bool {
        let Some((&entry, members)) = self.layout.split_first() else {
            return false;
        };
        if self.insts.len() > MOST_INSTRUCTIONS {
            return false;
        }
        // Nothing branches to the entry, so every target is a member.
        let members = members.to_vec();
        // A frame record's push is cold but runs on the first call of every
        // run that needs it, so it rejoins where it is.
        let frame_block = |ir: &Self, block: u32| {
            let insts = &ir.blocks[block as usize].insts;
            insts.iter().all(|inst| ir.frame_insts.contains(inst))
                && insts
                    .iter()
                    .any(|inst| matches!(ir.insts[*inst as usize].op, Op::Call(..)))
        };
        let exits_from = |ir: &Self, block: u32| {
            ir.blocks[block as usize].cold
                && !frame_block(ir, block)
                && ir
                    .terminator_targets(block)
                    .iter()
                    .any(|target| !ir.blocks[*target as usize].cold)
        };
        let exits = members
            .iter()
            .copied()
            .filter(|block| exits_from(self, *block))
            .collect::<Vec<_>>();
        if exits.is_empty() {
            return false;
        }
        self.rebuild_predecessors();
        let idom = self.dominators(entry);
        // Only a slow way after which warm code reads again what the way
        // to it read leaves the copy. Another rejoins where it is, since
        // leaving would only run the body as built from there.
        let slow = self.slow_blocks();
        let leaves = exits
            .into_iter()
            .filter(|exit| self.reads_again_after(*exit, &idom, &slow, imported))
            .collect::<HashSet<_>>();
        if leaves.is_empty() {
            return false;
        }
        let members = members
            .into_iter()
            .filter(|block| idom.contains_key(block))
            .collect::<Vec<_>>();
        // The back edges of the body as built, from its dominators, of the
        // loops a slow way leaves the copy in. A loop no slow way leaves
        // keeps its own back edges, so each body's copy of it stays a loop
        // with one entry, whose count LLVM can know.
        let back_edges = members
            .iter()
            .flat_map(|block| {
                self.terminator_targets(*block)
                    .into_iter()
                    .enumerate()
                    .filter(|(_, target)| {
                        dominates(&idom, *target, *block)
                            && self
                                .loop_blocks(*target, *block)
                                .any(|member| leaves.contains(&member))
                    })
                    .map(|(slot, _)| (*block, slot))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if recursive && back_edges.is_empty()
            || self.copy_growth(entry, &leaves, &back_edges) > MOST_GROWTH
        {
            return false;
        }
        let before = self.insts.len();

        let mut block_map = HashMap::new();
        let mut value_map = HashMap::new();
        for block in &members {
            let copy = self.blocks.len() as u32;
            let original = &self.blocks[*block as usize];
            let mut entry_ir = BlockIr {
                sealed: true,
                cold: original.cold,
                versioned: original.versioned,
                in_layout: true,
                version_decision: original.version_decision,
                ..BlockIr::default()
            };
            let params = original.params.clone();
            self.blocks.push(BlockIr::default());
            for param in params {
                let value = self.new_value(self.ty(param));
                value_map.insert(param, value);
                entry_ir.params.push(value);
            }
            self.blocks[copy as usize] = entry_ir;
            block_map.insert(*block, copy);
        }
        for block in &members {
            for inst in self.blocks[*block as usize].insts.clone() {
                for result in self.insts[inst as usize].results.clone() {
                    let value = self.new_value(self.ty(result));
                    value_map.insert(result, value);
                }
            }
        }
        for block in &members {
            let copy = block_map[block];
            let exits_here = leaves.contains(block);
            for inst in self.blocks[*block as usize].insts.clone() {
                let original = self.insts[inst as usize].clone();
                let value = |value: Val| value_map.get(&value).copied().unwrap_or(value);
                let target =
                    |target: Blk| Blk(block_map.get(&target.0).copied().unwrap_or(target.0));
                let mut op = map_op(&original.op, value, target);
                let index = self.insts.len() as u32;
                if exits_here && original.op.is_terminator() {
                    for (slot, first) in op_targets(&original.op).into_iter().enumerate() {
                        if !self.blocks[first.0 as usize].cold {
                            set_target(&mut op, slot, first);
                        }
                    }
                }
                if self.frame_insts.contains(&inst) {
                    self.frame_insts.insert(index);
                }
                let results = original
                    .results
                    .iter()
                    .map(|result| value_map[result])
                    .collect::<Vec<_>>();
                for result in &results {
                    self.value_defs[result.0 as usize] = Some(index);
                }
                self.insts.push(InstIr {
                    op,
                    results,
                    block: copy,
                    location: original.location,
                });
                self.blocks[copy as usize].insts.push(index);
            }
            self.layout.push(copy);
        }
        // The function runs the copy.
        let last = *self.blocks[entry as usize]
            .insts
            .last()
            .expect("a built body's entry ends in a branch");
        let entered = map_op(
            &self.insts[last as usize].op,
            |value| value,
            |target| Blk(block_map.get(&target.0).copied().unwrap_or(target.0)),
        );
        self.insts[last as usize].op = entered;
        // A loop of the first body goes back to the copy.
        for (block, slot) in back_edges {
            let last = *self.blocks[block as usize]
                .insts
                .last()
                .expect("a back edge's block ends in a branch");
            let own = op_targets(&self.insts[last as usize].op)[slot].0;
            set_target(
                &mut self.insts[last as usize].op,
                slot,
                Blk(block_map[&own]),
            );
        }
        // A value the moves between the bodies leave undefined on some way
        // to a use goes through a variable, which both copies of its
        // definition set.
        let twin = value_map
            .iter()
            .flat_map(|(first, copy)| [(*first, *copy), (*copy, *first)])
            .collect::<HashMap<_, _>>();
        self.demote_undominated(entry, &twin);
        self.rebuild_predecessors();
        self.slow_copy_insts = self.insts.len() - before;
        true
    }

    /// Whether warm code after the slow way `exit` rejoins reads again,
    /// before a call of an `imported` function, an indirect call, or a
    /// write of any memory, a word a block dominating
    /// `exit` read, or a word through a variable the slow way sets. The
    /// frame stack's words do not count, since every return reads them,
    /// and neither do a list's elements.
    fn reads_again_after(
        &self,
        exit: u32,
        idom: &HashMap<u32, u32>,
        slow: &HashSet<u32>,
        imported: &dyn Fn(FuncHandle) -> bool,
    ) -> bool {
        let mut read = HashSet::new();
        let mut block = exit;
        for _ in 0..READ_BLOCKS {
            let Some(&parent) = idom.get(&block).filter(|parent| **parent != block) else {
                break;
            };
            block = parent;
            for inst in &self.blocks[block as usize].insts {
                if let Op::Load(_, flags, address, offset) = &self.insts[*inst as usize].op {
                    if !matches!(
                        flags.alias,
                        AliasClass::FrameHeader | AliasClass::FrameEntry
                    ) {
                        read.insert(self.word(*address, *offset));
                    }
                }
            }
        }
        // The variables the slow way sets, whose values the copy knows
        // from its warm way once the slow way leaves it.
        let mut set_slow = self.defined_vars(exit);
        let mut seen = HashSet::new();
        let warm = |targets: Vec<u32>| {
            targets
                .into_iter()
                .filter(|target| !self.blocks[*target as usize].cold)
        };
        let mut work = warm(self.terminator_targets(exit)).collect::<Vec<_>>();
        while let Some(block) = work.pop() {
            if seen.len() >= READ_BLOCKS || !seen.insert(block) {
                continue;
            }
            // A block only slow ways reach is the slow way's own, whose
            // calls run before it rejoins.
            if slow.contains(&block) {
                set_slow.extend(self.defined_vars(block));
                work.extend(self.terminator_targets(block));
                continue;
            }
            for inst in &self.blocks[block as usize].insts {
                match &self.insts[*inst as usize].op {
                    Op::Load(_, flags, address, offset)
                        if flags.alias != AliasClass::ListElements =>
                    {
                        let word = self.word(*address, *offset);
                        if read.contains(&word)
                            || matches!(word.0, Word::Var(var) if set_slow.contains(&var))
                        {
                            return true;
                        }
                    }
                    // A call of a function of this module may be inlined.
                    Op::Call(callee, _) if imported(self.func_refs[callee.0 as usize]) => break,
                    Op::CallIndirect(..) => break,
                    Op::Store(flags, ..) if flags.alias == AliasClass::Any => break,
                    Op::Memory(access, ..) if *access != MemoryAccess::Read => break,
                    Op::Brif(..) | Op::Jump(..) => {
                        work.extend(warm(self.terminator_targets(block)))
                    }
                    _ => {}
                }
            }
        }
        false
    }

    /// The variables `block` sets.
    fn defined_vars(&self, block: u32) -> HashSet<u32> {
        self.blocks[block as usize]
            .insts
            .iter()
            .filter_map(|inst| match self.insts[*inst as usize].op {
                Op::DefVar(var, _) => Some(var.0),
                _ => None,
            })
            .collect()
    }

    /// The cold blocks, and the blocks whose every predecessor is one.
    fn slow_blocks(&self) -> HashSet<u32> {
        let mut slow = self
            .layout
            .iter()
            .copied()
            .filter(|block| self.blocks[*block as usize].cold)
            .collect::<HashSet<_>>();
        let mut changed = true;
        while changed {
            changed = false;
            for block in &self.layout {
                let predecessors = &self.blocks[*block as usize].predecessors;
                if !predecessors.is_empty()
                    && predecessors
                        .iter()
                        .all(|predecessor| slow.contains(predecessor))
                    && slow.insert(*block)
                {
                    changed = true;
                }
            }
        }
        slow
    }

    /// The value or variable and byte offset a load of `offset` from
    /// `address` reads, through the constant offsets added to the value.
    fn word(&self, mut address: Val, offset: i32) -> (Word, i64) {
        let mut offset = i64::from(offset);
        while let Some(def) = self.value_defs[address.0 as usize] {
            match &self.insts[def as usize].op {
                Op::BinaryImm(BinaryOp::Iadd, base, added) => {
                    address = *base;
                    offset += added;
                }
                Op::Copy(value) => address = *value,
                Op::UseVar(var) => return (Word::Var(var.0), offset),
                _ => break,
            }
        }
        (Word::Value(address), offset)
    }

    /// How many more recorded instructions the reachable blocks hold once
    /// the function runs the copy, the slow ways of `leaves` leave it, and
    /// the back edges `back_edges` of the first body go back to it.
    fn copy_growth(&self, entry: u32, leaves: &HashSet<u32>, back_edges: &[(u32, usize)]) -> usize {
        let back_edges = back_edges.iter().copied().collect::<HashSet<_>>();
        let size = |block: u32| self.blocks[block as usize].insts.len();
        let mut before = 0;
        let mut seen = HashSet::from([entry]);
        let mut work = vec![entry];
        while let Some(block) = work.pop() {
            before += size(block);
            for target in self.terminator_targets(block) {
                if seen.insert(target) {
                    work.push(target);
                }
            }
        }
        // Each block of the first body (false) or of the copy (true).
        let mut after = 0;
        let mut seen = HashSet::from([(entry, false)]);
        let mut work = vec![(entry, false)];
        while let Some((block, in_copy)) = work.pop() {
            after += size(block);
            for (slot, target) in self.terminator_targets(block).into_iter().enumerate() {
                let to_copy = if in_copy || block == entry {
                    !leaves.contains(&block) || self.blocks[target as usize].cold
                } else {
                    back_edges.contains(&(block, slot))
                };
                if seen.insert((target, to_copy)) {
                    work.push((target, to_copy));
                }
            }
        }
        after - before
    }

    /// Whether the body calls `function`.
    pub(super) fn calls(&self, function: FuncHandle) -> bool {
        self.insts.iter().any(|inst| {
            matches!(&inst.op, Op::Call(callee, _) if self.func_refs[callee.0 as usize] == function)
        })
    }

    /// The recorded instructions of the body as built that are not a
    /// deferred frame's: the copy and its frame instructions weigh nothing.
    pub(super) fn weight(&self) -> usize {
        let built = self.insts.len() - self.slow_copy_insts;
        let frames = self
            .frame_insts
            .iter()
            .filter(|inst| (**inst as usize) < built)
            .count();
        built - frames
    }

    /// The blocks the branch that ends `block` goes to.
    fn terminator_targets(&self, block: u32) -> Vec<u32> {
        let last = self.blocks[block as usize]
            .insts
            .last()
            .expect("a block of the layout ends in a terminator");
        op_targets(&self.insts[*last as usize].op)
            .into_iter()
            .map(|target| target.0)
            .collect()
    }

    /// The blocks of the loop that the back edge from `latch` to `header`
    /// closes: the header, and each block from which the latch is reached
    /// without passing the header.
    fn loop_blocks(&self, header: u32, latch: u32) -> impl Iterator<Item = u32> {
        let mut members = HashSet::from([header, latch]);
        let mut work = vec![latch];
        while let Some(block) = work.pop() {
            if block == header {
                continue;
            }
            for predecessor in &self.blocks[block as usize].predecessors {
                if members.insert(*predecessor) {
                    work.push(*predecessor);
                }
            }
        }
        members.into_iter()
    }

    /// The blocks reachable from `entry` in reverse postorder.
    fn reverse_postorder(&self, entry: u32) -> Vec<u32> {
        let mut order = Vec::new();
        let mut seen = HashSet::from([entry]);
        let mut work = vec![(entry, 0usize)];
        while let Some((block, next)) = work.pop() {
            let targets = self.terminator_targets(block);
            if let Some(&target) = targets.get(next) {
                work.push((block, next + 1));
                if seen.insert(target) {
                    work.push((target, 0));
                }
            } else {
                order.push(block);
            }
        }
        order.reverse();
        order
    }

    /// The immediate dominator of each block reachable from `entry`
    /// (Cooper, Harvey, and Kennedy's iteration). The entry is its own.
    fn dominators(&self, entry: u32) -> HashMap<u32, u32> {
        let order = self.reverse_postorder(entry);
        let position = order
            .iter()
            .enumerate()
            .map(|(index, block)| (*block, index))
            .collect::<HashMap<_, _>>();
        let mut predecessors = HashMap::<u32, Vec<u32>>::new();
        for block in &order {
            for target in self.terminator_targets(*block) {
                predecessors.entry(target).or_default().push(*block);
            }
        }
        let mut idom = HashMap::from([(entry, entry)]);
        let mut changed = true;
        while changed {
            changed = false;
            for block in order.iter().skip(1) {
                let mut new = None;
                for predecessor in predecessors.get(block).into_iter().flatten() {
                    if !idom.contains_key(predecessor) {
                        continue;
                    }
                    new = Some(match new {
                        None => *predecessor,
                        Some(current) => {
                            let (mut left, mut right) = (current, *predecessor);
                            while left != right {
                                while position[&left] > position[&right] {
                                    left = idom[&left];
                                }
                                while position[&right] > position[&left] {
                                    right = idom[&right];
                                }
                            }
                            left
                        }
                    });
                }
                // A block in reverse postorder comes after one of its
                // predecessors.
                let new = new.expect("a reached block has a reached predecessor");
                if idom.insert(*block, new) != Some(new) {
                    changed = true;
                }
            }
        }
        idom
    }

    /// Gives each value whose definition no longer lies on every way to
    /// one of its uses, or that both bodies define and a block other than
    /// its own uses, a variable: both copies of the definition set it, and
    /// each such use reads it. Each way to the use passes one of the two
    /// copies last, since in the body as built the definition lies on
    /// every way to the use, so the variable holds the value the use
    /// would have read there, even where a way passes one copy and then
    /// the other.
    fn demote_undominated(&mut self, entry: u32, twin: &HashMap<Val, Val>) {
        let idom = self.dominators(entry);
        let mut param_blocks = HashMap::new();
        for block in &self.layout {
            for param in &self.blocks[*block as usize].params {
                param_blocks.insert(*param, *block);
            }
        }
        // Every value is an instruction's result or a block's parameter.
        let def_block = |ir: &Self, value: Val| match ir.value_defs[value.0 as usize] {
            Some(def) => ir.insts[def as usize].block,
            None => param_blocks[&value],
        };
        // Each use to rewrite, as its instruction and the value it reads.
        let mut uses = Vec::new();
        let mut seen = HashSet::new();
        for block in &self.layout {
            if !idom.contains_key(block) {
                continue;
            }
            for inst in &self.blocks[*block as usize].insts {
                for operand in op_operands(&self.insts[*inst as usize].op) {
                    let def = def_block(self, operand);
                    // A value both bodies define may be defined again by
                    // its twin between its definition and a use in another
                    // block, even where its definition dominates the use.
                    let crosses = def != *block && twin.contains_key(&operand);
                    if (crosses || !dominates(&idom, def, *block)) && seen.insert((*inst, operand))
                    {
                        uses.push((*inst, operand));
                    }
                }
            }
        }
        if uses.is_empty() {
            return;
        }
        let mut next_var = self.vars.keys().next_back().map_or(0, |last| last + 1);
        let mut vars = HashMap::<Val, Var>::new();
        for (_, value) in &uses {
            if vars.contains_key(value) {
                continue;
            }
            let var = Var(next_var);
            next_var += 1;
            self.vars.insert(var.0, self.ty(*value));
            for defined in std::iter::once(*value).chain(twin.get(value).copied()) {
                vars.insert(defined, var);
                self.set_after_definition(defined, var, &param_blocks);
            }
        }
        for (inst, value) in uses {
            let var = vars[&value];
            let block = self.insts[inst as usize].block;
            let position = self.blocks[block as usize]
                .insts
                .iter()
                .position(|candidate| *candidate == inst)
                .expect("a use lies in its block");
            let read = self.new_value(self.ty(value));
            let index = self.insts.len() as u32;
            self.value_defs[read.0 as usize] = Some(index);
            let location = self.insts[inst as usize].location;
            self.insts.push(InstIr {
                op: Op::UseVar(var),
                results: vec![read],
                block,
                location,
            });
            self.blocks[block as usize].insts.insert(position, index);
            // A frame record's push reads its operands as part of it.
            if self.frame_insts.contains(&inst) {
                self.frame_insts.insert(index);
            }
            let op = map_op(
                &self.insts[inst as usize].op,
                |operand| if operand == value { read } else { operand },
                |target| target,
            );
            self.insts[inst as usize].op = op;
        }
    }

    /// Stores `value` into `var` right where it is defined.
    fn set_after_definition(&mut self, value: Val, var: Var, param_blocks: &HashMap<Val, u32>) {
        let (block, position) = match self.value_defs[value.0 as usize] {
            Some(def) => {
                let block = self.insts[def as usize].block;
                let position = self.blocks[block as usize]
                    .insts
                    .iter()
                    .position(|candidate| *candidate == def)
                    .expect("a definition lies in its block");
                (block, position + 1)
            }
            None => (param_blocks[&value], 0),
        };
        let index = self.insts.len() as u32;
        let location = self.blocks[block as usize]
            .insts
            .get(position.saturating_sub(1))
            .and_then(|inst| self.insts[*inst as usize].location);
        self.insts.push(InstIr {
            op: Op::DefVar(var, value),
            results: Vec::new(),
            block,
            location,
        });
        self.blocks[block as usize].insts.insert(position, index);
    }

    /// Sets each block's predecessors from the branches of the layout.
    fn rebuild_predecessors(&mut self) {
        for block in &mut self.blocks {
            block.predecessors.clear();
        }
        for block in self.layout.clone() {
            for target in self.terminator_targets(block) {
                self.blocks[target as usize].predecessors.push(block);
            }
        }
    }
}

/// What an address is computed from: a value, or a read of a variable.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Word {
    Value(Val),
    Var(u32),
}

/// Whether `block` lies under `def` in the dominator tree `idom`.
fn dominates(idom: &HashMap<u32, u32>, def: u32, block: u32) -> bool {
    let mut current = block;
    loop {
        if current == def {
            return true;
        }
        match idom.get(&current) {
            Some(&parent) if parent != current => current = parent,
            // The entry, or a block no way reaches.
            _ => return false,
        }
    }
}

/// Points target `slot` of the branch `op` at `target`.
fn set_target(op: &mut Op, slot: usize, target: Blk) {
    let seen = std::cell::Cell::new(0);
    *op = map_op(
        op,
        |value| value,
        |block| {
            let index = seen.replace(seen.get() + 1);
            if index == slot {
                target
            } else {
                block
            }
        },
    );
}

#[cfg(test)]
#[path = "native_backend_llvm_slow_paths_tests.rs"]
mod tests;
