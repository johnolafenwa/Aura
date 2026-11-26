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
//! Every block of the copy computes what its block computes, from the
//! copy's values, and the variables are the same memory in both, so a
//! move between the two at any edge is exact as long as each value is
//! defined on every way to each of its uses. An edge that would break
//! that keeps its target in its own body.

use std::collections::{HashMap, HashSet};

use super::{map_op, op_operands, op_targets, BlockIr, FunctionIr, InstIr, Op, Val, Var};
use crate::native_backend::Blk;

/// A body of more recorded instructions gets no copy.
const MOST_INSTRUCTIONS: usize = 24_000;

impl FunctionIr {
    /// Gives the body a copy that the function runs, whose slow paths
    /// continue in the body as it was (see the module documentation).
    /// Returns whether the copy was made: a body with no slow path that
    /// rejoins a warm block, or one too large, stays as it is.
    pub(super) fn continue_slow_paths(&mut self) -> bool {
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
        if !members.iter().any(|block| exits_from(self, *block)) {
            return false;
        }
        let exits = members
            .iter()
            .copied()
            .filter(|block| exits_from(self, *block))
            .collect::<HashSet<_>>();
        // The back edges of the body as built, from its dominators, of the
        // loops a slow way leaves the copy in. A loop no slow way leaves
        // keeps its own back edges, so each body's copy of it stays a loop
        // with one entry, whose count LLVM can know.
        self.rebuild_predecessors();
        let idom = self.dominators(entry);
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
                                .any(|member| exits.contains(&member))
                    })
                    .map(|(slot, _)| (*block, slot))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
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
            let exits_here = exits.contains(block);
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
    /// one of its uses a variable: both copies of the definition set it,
    /// and each such use reads it. Each way to the use passes one of the
    /// two copies last, since in the body as built the definition lies on
    /// every way to the use, so the variable holds the value the use
    /// would have read there.
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
                    if !dominates(&idom, def, *block) && seen.insert((*inst, operand)) {
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
