//! One read of the task's frame stack address for the calls it reaches.
//!
//! Generated code asks the runtime for the address of the running task's
//! frame stack (`aura_direct_frame_stack`) wherever it needs it: to push a
//! frame record, at a safepoint, and before every inline retain and
//! release, which compare a value's owner with it. The import is declared
//! `memory(read)`, so LLVM merges two calls only when nothing between them
//! may write memory, and nearly every runtime call may. It must not be
//! declared `memory(none)`: a `control.recover` call makes another stack
//! the task's current one between its begin and its end, and a generator's
//! body is resumed on the stack of whichever call resumes it, so a call
//! made before such a switch must not stand in for one made after it.
//!
//! In a function that calls nothing that switches stacks, every call made
//! by one activation returns the same address: any other call it makes,
//! to the runtime or to Aura code, returns with the stack that was current
//! when it was made. [`share_frame_stacks`] finds those functions by the
//! names of the functions they call, and in each replaces a call of the
//! accessor by an earlier one that dominates it. The value is that of a
//! call the same activation made, so the replacement stays exact when LLVM
//! later inlines the function into one that does switch stacks.

use std::collections::HashMap;

use inkwell::basic_block::BasicBlock;
use inkwell::llvm_sys::core::{LLVMGetNumSuccessors, LLVMGetSuccessor};
use inkwell::module::Module;
use inkwell::values::{
    AsValueRef, CallSiteValue, FunctionValue, InstructionOpcode, InstructionValue,
};

use crate::runtime_abi::{RuntimeFn, RECOVER_SET_JUMP};

/// Whether a call of the function `name` may return with another frame
/// stack current than the one it was called on, or returns twice: the
/// calls that begin, end, and resume a recovered call, and the host calls
/// of a generator, whose body runs on the stack of the call that resumes
/// it.
pub(super) fn switches_frame_stack(name: &str) -> bool {
    name.starts_with("aura_direct_recover_")
        || name.starts_with("aura_host_iter_generator_")
        || name == RECOVER_SET_JUMP
}

/// Replaces, in every function of `module` that calls nothing that
/// switches frame stacks, each call of the frame stack accessor that an
/// earlier one dominates by that one (see the module documentation).
/// Returns how many calls it removed.
pub(super) fn share_frame_stacks(module: &Module<'_>) -> usize {
    let Some(accessor) = module.get_function(RuntimeFn::FrameStack.symbol()) else {
        return 0;
    };
    let mut removed = 0;
    let mut function = module.get_first_function();
    while let Some(current) = function {
        removed += share_in_function(current, accessor);
        function = current.get_next_function();
    }
    removed
}

/// The function a call instruction calls by name, if it is one.
fn direct_callee<'ctx>(instruction: InstructionValue<'ctx>) -> Option<FunctionValue<'ctx>> {
    if instruction.get_opcode() != InstructionOpcode::Call {
        return None;
    }
    CallSiteValue::try_from(instruction)
        .ok()?
        .get_called_fn_value()
}

fn share_in_function<'ctx>(function: FunctionValue<'ctx>, accessor: FunctionValue<'ctx>) -> usize {
    let blocks = function.get_basic_blocks();
    let mut reads = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        for instruction in block.get_instructions() {
            let Some(callee) = direct_callee(instruction) else {
                continue;
            };
            if callee == accessor {
                reads.push((index, instruction));
            } else if switches_frame_stack(&callee.get_name().to_string_lossy()) {
                return 0;
            }
        }
    }
    if reads.len() < 2 {
        return 0;
    }
    let dominators = Dominators::of(&blocks);
    // The reads in an order in which a dominating read comes first: by
    // their blocks' places in reverse postorder, then as their block has
    // them. A read in a block nothing reaches stays as it is.
    let mut ordered = reads
        .into_iter()
        .filter_map(|(block, read)| Some((dominators.order[block]?, block, read)))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(order, _, _)| *order);
    let mut kept: Vec<(usize, InstructionValue<'ctx>)> = Vec::new();
    let mut removed = 0;
    for (_, block, read) in ordered {
        match kept
            .iter()
            .find(|(leader, _)| dominators.dominates(*leader, block))
        {
            Some((_, leader)) => {
                read.replace_all_uses_with(leader);
                read.erase_from_basic_block();
                removed += 1;
            }
            None => kept.push((block, read)),
        }
    }
    removed
}

/// The dominator tree of a function's blocks, by their indices in the
/// function. The entry block is block 0.
struct Dominators {
    /// Each block's place in reverse postorder, or `None` for a block
    /// nothing reaches.
    order: Vec<Option<usize>>,
    /// Each reached block's immediate dominator. The entry is its own.
    immediate: Vec<usize>,
}

impl Dominators {
    fn of(blocks: &[BasicBlock<'_>]) -> Self {
        let indices = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.as_mut_ptr(), index))
            .collect::<HashMap<_, _>>();
        let successors = blocks
            .iter()
            .map(|block| {
                let Some(terminator) = block.get_terminator() else {
                    return Vec::new();
                };
                // SAFETY: `terminator` is a live terminator instruction, and
                // each index is below its successor count.
                unsafe {
                    let value = terminator.as_value_ref();
                    (0..LLVMGetNumSuccessors(value))
                        .filter_map(|at| indices.get(&LLVMGetSuccessor(value, at)).copied())
                        .collect::<Vec<_>>()
                }
            })
            .collect::<Vec<_>>();
        Self::of_successors(&successors)
    }

    /// The tree of the graph whose block `index` goes to
    /// `successors[index]`, by the iteration of Cooper, Harvey, and
    /// Kennedy.
    fn of_successors(successors: &[Vec<usize>]) -> Self {
        let count = successors.len();
        let mut postorder = Vec::with_capacity(count);
        let mut seen = vec![false; count];
        let mut stack = Vec::new();
        if count > 0 {
            seen[0] = true;
            stack.push((0usize, 0usize));
        }
        while let Some((block, next)) = stack.last_mut() {
            match successors[*block].get(*next).copied() {
                Some(successor) => {
                    *next += 1;
                    if !seen[successor] {
                        seen[successor] = true;
                        stack.push((successor, 0));
                    }
                }
                None => {
                    postorder.push(*block);
                    stack.pop();
                }
            }
        }
        let mut order = vec![None; count];
        for (place, block) in postorder.iter().rev().enumerate() {
            order[*block] = Some(place);
        }
        let mut predecessors = vec![Vec::new(); count];
        for (block, targets) in successors.iter().enumerate() {
            if order[block].is_some() {
                for target in targets {
                    predecessors[*target].push(block);
                }
            }
        }
        const NONE: usize = usize::MAX;
        let mut immediate = vec![NONE; count];
        if count > 0 {
            immediate[0] = 0;
        }
        let place = |block: usize| order[block].expect("a reached block has a place");
        let mut changed = true;
        while changed {
            changed = false;
            for block in postorder.iter().rev().copied().skip(1) {
                let mut found = NONE;
                for predecessor in predecessors[block].iter().copied() {
                    if immediate[predecessor] == NONE {
                        continue;
                    }
                    if found == NONE {
                        found = predecessor;
                        continue;
                    }
                    let (mut left, mut right) = (predecessor, found);
                    while left != right {
                        while place(left) > place(right) {
                            left = immediate[left];
                        }
                        while place(right) > place(left) {
                            right = immediate[right];
                        }
                    }
                    found = left;
                }
                if immediate[block] != found {
                    immediate[block] = found;
                    changed = true;
                }
            }
        }
        Self { order, immediate }
    }

    /// Whether every way from the entry to `block` passes `dominator`. A
    /// block dominates itself.
    fn dominates(&self, dominator: usize, block: usize) -> bool {
        let mut at = block;
        loop {
            if at == dominator {
                return true;
            }
            let up = self.immediate[at];
            if up == at {
                return false;
            }
            at = up;
        }
    }
}

#[cfg(test)]
#[path = "native_backend_llvm_frame_stacks_tests.rs"]
mod tests;
