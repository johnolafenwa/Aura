//! The ownership checker (Batch 7 step 8e).
//!
//! An explicit module states every reference a value gains or loses. The
//! checker walks each function forward and tracks each root local whose
//! value needs drop:
//!
//! | State | Meaning |
//! |---|---|
//! | Empty | holds no value |
//! | Owned | holds a value |
//! | Maybe | holds a value on some paths only |
//!
//! Joining Owned with Empty gives Maybe, but a path that never stored the
//! root adds nothing. A use or a retain needs Owned, a
//! release needs Owned or Maybe, and a store to a root needs Empty. Each
//! consuming `Place` operand takes one earlier `Retain`, and none is left
//! over. A return leaves every root empty except `mut` parameters and the
//! root it returns from. So each value is released exactly once on every
//! path.
//!
//! The checker first finds each block's entry state, then checks each
//! block once, so a state seen only on the way to the fixed point never
//! reports an error.

use std::collections::BTreeSet;

use super::ownership::{
    edge_binding, root_of, terminator_accesses, Access, FlowGraph, OwnershipFacts, Touch,
    MAX_OWNERSHIP_STATE_CELLS,
};
use super::{
    Instruction, MirFunction, MirLoanValidationContext, MirModule, MirOwnership, Operand, Rvalue,
    Terminator,
};

/// What one tracked root holds.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Hold {
    /// Empty: nothing was stored yet.
    Unset,
    /// Empty: a release dropped the value.
    Released,
    /// Empty: an operation moved the value out.
    Moved,
    Owned,
    Maybe,
}

impl Hold {
    fn is_empty(self) -> bool {
        matches!(self, Hold::Unset | Hold::Released | Hold::Moved)
    }

    /// A path that never stored the root adds nothing: definite
    /// assignment is the type checker's rule, and lowering leaves edges no
    /// run takes, such as a `match` falling past its last arm.
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (left, right) if left == right => left,
            (Hold::Unset, other) | (other, Hold::Unset) => other,
            (Hold::Released | Hold::Moved, Hold::Released | Hold::Moved) => self.min(other),
            _ => Hold::Maybe,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct State<'f> {
    holds: Vec<Hold>,
    /// Roots with a registered cleanup.
    cleanups: BTreeSet<usize>,
    /// Active loans.
    loans: BTreeSet<&'f str>,
}

impl State<'_> {
    /// Joins `other` in and says whether anything changed.
    fn join(&mut self, other: &Self) -> bool {
        let before = self.clone();
        for (hold, other) in self.holds.iter_mut().zip(&other.holds) {
            *hold = hold.join(*other);
        }
        self.cleanups.extend(other.cleanups.iter().copied());
        self.loans.extend(other.loans.iter().copied());
        *self != before
    }
}

/// Checks the ownership an explicit module states. An implicit module
/// states none, so it passes.
pub(crate) fn validate_ownership(module: &MirModule) -> Result<(), String> {
    validate_ownership_within(module, MAX_OWNERSHIP_STATE_CELLS, None)
}

/// [`validate_ownership`] with its budgets: at most `state_cells` tracked
/// roots times reachable blocks per function, and at most `steps` block
/// visits on the way to the fixed point. Without `steps`, the bound the
/// lattice gives applies: each root's state, each cleanup, and each loan
/// changes a block's entry a bounded number of times.
pub(crate) fn validate_ownership_within(
    module: &MirModule,
    state_cells: usize,
    steps: Option<usize>,
) -> Result<(), String> {
    if module.ownership == MirOwnership::Implicit {
        return Ok(());
    }
    let context = MirLoanValidationContext::new(module)?;
    for function in module.functions.iter().chain(module.top_level.iter()) {
        check_function(function, &context, state_cells, steps)?;
    }
    Ok(())
}

fn states_ownership(function: &MirFunction) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| match instruction {
            Instruction::Retain { .. } | Instruction::Release { .. } => true,
            Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => matches!(
                value,
                Rvalue::Try { releases_on_error, .. } if !releases_on_error.is_empty()
            ),
            _ => false,
        })
}

pub(super) fn check_function(
    function: &MirFunction,
    context: &MirLoanValidationContext<'_>,
    state_cells: usize,
    steps: Option<usize>,
) -> Result<(), String> {
    let facts = OwnershipFacts::new(function, context);
    if facts.roots.is_empty() && !states_ownership(function) {
        return Ok(());
    }
    let graph = FlowGraph::new(function, context).ok_or_else(|| {
        format!(
            "invalid MIR function `{}` has a block graph the ownership checker cannot read",
            function.name
        )
    })?;
    let reachable = graph.reachable_count();
    if facts.roots.len().saturating_mul(reachable) > state_cells {
        return Err(format!(
            "invalid MIR function `{}` is too large for the ownership checker",
            function.name
        ));
    }
    let loan_count = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter(|instruction| {
            matches!(
                instruction,
                Instruction::BeginLoan { .. }
                    | Instruction::BeginElementLoan { .. }
                    | Instruction::BeginReturnedLoan { .. }
                    | Instruction::Reborrow { .. }
                    | Instruction::ReborrowElement { .. }
            )
        })
        .count();
    let budget = steps.unwrap_or_else(|| {
        reachable.saturating_mul(
            2usize
                .saturating_add(facts.roots.len().saturating_mul(4))
                .saturating_add(loan_count),
        )
    });
    let mut checker = Checker {
        context,
        function,
        entry: Vec::new(),
        facts,
        pending: Vec::new(),
        error: None,
        report: false,
    };
    checker.entry_states(&graph, budget)?;
    checker.report = true;
    for (block, reachable) in graph.reachable.iter().enumerate() {
        if !*reachable {
            continue;
        }
        let mut state = checker.entry[block]
            .clone()
            .expect("the fixed point reaches every reachable block");
        checker.walk(block, &mut state);
        if let Some(error) = checker.error.take() {
            return Err(error);
        }
    }
    Ok(())
}

struct Checker<'f, 'c> {
    context: &'c MirLoanValidationContext<'c>,
    function: &'f MirFunction,
    facts: OwnershipFacts<'f>,
    entry: Vec<Option<State<'f>>>,
    /// Retains no operand has taken yet.
    pending: Vec<&'f str>,
    /// The first error found while `report` is on.
    error: Option<String>,
    report: bool,
}

impl<'f> Checker<'f, '_> {
    fn fail(&mut self, message: impl FnOnce() -> String) {
        if self.report && self.error.is_none() {
            self.error = Some(format!(
                "invalid MIR in `{}` {}",
                self.function.name,
                message()
            ));
        }
    }

    fn entry_states(&mut self, graph: &FlowGraph, budget: usize) -> Result<(), String> {
        let width = self.facts.roots.len();
        let mut start = State {
            holds: vec![Hold::Unset; width],
            cleanups: BTreeSet::new(),
            loans: BTreeSet::new(),
        };
        for root in self.facts.entry.iter() {
            start.holds[root] = Hold::Owned;
        }
        self.entry = vec![None; self.function.blocks.len()];
        self.entry[graph.entry] = Some(start);
        let mut pending = vec![graph.entry];
        let mut queued = vec![false; self.function.blocks.len()];
        queued[graph.entry] = true;
        let mut visits = 0usize;
        while let Some(block) = pending.pop() {
            queued[block] = false;
            visits += 1;
            if visits > budget {
                return Err(format!(
                    "invalid MIR function `{}` exceeds the ownership checker's step budget",
                    self.function.name
                ));
            }
            let mut state = self.entry[block]
                .clone()
                .expect("a queued block has an entry state");
            self.walk(block, &mut state);
            for &successor in &graph.successors[block] {
                let mut outgoing = state.clone();
                if let Some(root) = edge_binding(self.function, block, successor)
                    .and_then(|binding| self.facts.tracked(binding))
                {
                    outgoing.holds[root] = Hold::Owned;
                }
                let changed = match &mut self.entry[successor] {
                    Some(existing) => existing.join(&outgoing),
                    slot @ None => {
                        *slot = Some(outgoing);
                        true
                    }
                };
                if changed && !std::mem::replace(&mut queued[successor], true) {
                    pending.push(successor);
                }
            }
        }
        Ok(())
    }

    /// Walks one block from `state`, which ends as the state the
    /// terminator leaves.
    fn walk(&mut self, block: usize, state: &mut State<'f>) {
        let function = self.function;
        let block = &function.blocks[block];
        self.pending.clear();
        for instruction in &block.instructions {
            self.instruction(instruction, state);
        }
        for (place, access) in terminator_accesses(&block.terminator) {
            self.access(state, place, access);
        }
        if let Terminator::Return(value) = &block.terminator {
            self.exit(state, value);
        }
        self.leftover_retains();
    }

    fn instruction(&mut self, instruction: &'f Instruction, state: &mut State<'f>) {
        match instruction {
            Instruction::Retain { place } => return self.retain(state, place),
            Instruction::Release { place } => self.release(state, place),
            _ => {
                if let Instruction::Assign {
                    value: Rvalue::StartTask { function, args, .. },
                    ..
                } = instruction
                {
                    self.task_arguments(
                        std::iter::once(function).chain(args.iter().map(|arg| &arg.value)),
                    );
                }
                let try_releases = match instruction {
                    Instruction::Assign {
                        value:
                            Rvalue::Try {
                                releases_on_error, ..
                            },
                        ..
                    }
                    | Instruction::WriteLoan {
                        value:
                            Rvalue::Try {
                                releases_on_error, ..
                            },
                        ..
                    } => Some(releases_on_error),
                    _ => None,
                };
                let accesses = self.facts.accesses(instruction);
                let count = accesses.len();
                for (position, (place, access)) in accesses.into_iter().enumerate() {
                    // A `try` releases on its error edge once its operand is
                    // taken and before its target is stored.
                    if position + 1 == count {
                        if let Some(listed) = try_releases {
                            self.try_releases(state, listed);
                        }
                    }
                    self.access(state, place, access);
                }
                self.scopes(instruction, state);
            }
        }
        self.leftover_retains();
    }

    fn leftover_retains(&mut self) {
        if let Some(place) = self.pending.first().copied() {
            self.fail(|| format!("leaves a retain of `{place}` that no operand takes"));
        }
        self.pending.clear();
    }

    fn task_arguments<'a>(&mut self, operands: impl Iterator<Item = &'a Operand>) {
        for operand in operands {
            if let Operand::Place(place) = operand {
                if self.pending.contains(&place.as_str()) {
                    self.fail(|| {
                        format!("retains task argument `{place}`, but a task copies its arguments")
                    });
                }
            }
        }
    }

    /// Tracks the cleanups and loans an instruction registers or ends.
    fn scopes(&mut self, instruction: &'f Instruction, state: &mut State<'f>) {
        match instruction {
            Instruction::PushCleanup { place, .. } => {
                if let Some(root) = self.facts.tracked(root_of(place)) {
                    state.cleanups.insert(root);
                }
            }
            Instruction::PopCleanup { place, .. } => {
                if let Some(root) = self.facts.tracked(root_of(place)) {
                    state.cleanups.remove(&root);
                }
            }
            Instruction::BeginLoan { loan, .. }
            | Instruction::BeginElementLoan { loan, .. }
            | Instruction::BeginReturnedLoan { loan, .. }
            | Instruction::Reborrow { loan, .. }
            | Instruction::ReborrowElement { loan, .. } => {
                state.loans.insert(loan);
            }
            Instruction::EndLoan { loan } => {
                state.loans.remove(loan.as_str());
            }
            Instruction::ReturnLoan { loan, .. } => {
                state.loans.remove(root_of(loan));
            }
            _ => {}
        }
    }

    fn retain(&mut self, state: &State<'f>, place: &'f str) {
        let touch = self.facts.touch(self.context, place, Access::Read);
        if touch == Touch::Untracked || !self.facts.place_needs_drop(self.context, place) {
            self.fail(|| format!("retains `{place}`, whose value needs no drop"));
        } else if let Touch::Tracked { root, .. } = touch {
            self.holding(state, root, place, "retains");
        }
        self.pending.push(place);
    }

    fn release(&mut self, state: &mut State<'f>, place: &'f str) {
        let Some(root) = self
            .facts
            .tracked(place)
            .filter(|_| root_of(place).len() == place.len())
        else {
            return self.fail(|| {
                format!("releases `{place}`, which is not a root whose value needs drop")
            });
        };
        match state.holds[root] {
            Hold::Owned | Hold::Maybe => {}
            Hold::Unset => self.fail(|| format!("releases `{place}` before it holds a value")),
            Hold::Released => self.fail(|| format!("releases `{place}` twice")),
            Hold::Moved => self.fail(|| format!("releases `{place}` after moving it")),
        }
        self.unlocked(state, root, place, "releases");
        state.holds[root] = Hold::Released;
    }

    /// A release or a move needs no cleanup registered on the root and no
    /// active loan of it.
    fn unlocked(&mut self, state: &State<'f>, root: usize, place: &str, verb: &str) {
        if state.cleanups.contains(&root) {
            self.fail(|| format!("{verb} `{place}` while its cleanup is registered"));
        }
        let facts = &self.facts;
        if let Some(loan) = state
            .loans
            .iter()
            .find(|loan| facts.borrowed_by(loan).contains(&root))
            .copied()
        {
            self.fail(|| format!("{verb} `{place}` while loan `{loan}` borrows it"));
        }
    }

    fn holding(&mut self, state: &State<'f>, root: usize, place: &str, verb: &str) {
        match state.holds[root] {
            Hold::Owned => {}
            Hold::Maybe => {
                self.fail(|| format!("{verb} `{place}`, which holds a value on only some paths"))
            }
            Hold::Unset => self.fail(|| format!("{verb} `{place}` before it holds a value")),
            Hold::Released => self.fail(|| format!("{verb} `{place}` after releasing it")),
            Hold::Moved => self.fail(|| format!("{verb} `{place}` after moving it")),
        }
    }

    fn access(&mut self, state: &mut State<'f>, place: &'f str, access: Access) {
        let (root, whole, retained) = match self.facts.touch(self.context, place, access) {
            Touch::Tracked {
                root,
                whole,
                retained,
            } => (Some(root), whole, retained),
            Touch::Loan { retained } => (None, false, retained),
            Touch::Untracked => (None, false, false),
        };
        if let Some(root) = root {
            match (access, whole) {
                (Access::Assign, true) => {
                    match state.holds[root] {
                        Hold::Owned => {
                            self.fail(|| format!("assigns `{place}` while it still holds a value"))
                        }
                        Hold::Maybe => self.fail(|| {
                            format!("assigns `{place}`, which holds a value on only some paths")
                        }),
                        _ => {}
                    }
                    state.holds[root] = Hold::Owned;
                }
                (Access::Writeback, true) => state.holds[root] = Hold::Owned,
                (Access::Move, true) => {
                    self.holding(state, root, place, "moves");
                    self.unlocked(state, root, place, "moves");
                    state.holds[root] = Hold::Moved;
                }
                (Access::Read | Access::Consume, _) => self.holding(state, root, place, "uses"),
                (Access::Move | Access::TakePart, _) => self.holding(state, root, place, "moves"),
                (Access::Assign | Access::Writeback, false) => {
                    self.holding(state, root, place, "stores into")
                }
            }
        }
        if retained {
            match self.pending.iter().position(|pending| *pending == place) {
                Some(position) => {
                    self.pending.remove(position);
                }
                None => self.fail(|| format!("consumes `{place}` without a retain")),
            }
        }
    }

    fn try_releases(&mut self, state: &State<'f>, listed: &[String]) {
        let expected = state
            .holds
            .iter()
            .enumerate()
            .filter(|(root, hold)| !hold.is_empty() && !self.facts.mutable.contains(*root))
            .map(|(root, _)| self.facts.roots[root])
            .collect::<Vec<_>>();
        if listed
            .iter()
            .map(String::as_str)
            .ne(expected.iter().copied())
        {
            self.fail(|| {
                format!(
                    "lists `try` releases [{}], but its error edge must release [{}]",
                    listed.join(", "),
                    expected.join(", ")
                )
            });
        }
    }

    /// A return hands `mut` parameters back and releases the root it
    /// returns from. Every other root must be empty.
    fn exit(&mut self, state: &State<'f>, value: &Operand) {
        let returned = match value {
            Operand::Place(place) | Operand::MovePlace(place) => self.facts.tracked(root_of(place)),
            _ => None,
        };
        for root in 0..state.holds.len() {
            let name = self.facts.roots[root];
            let hold = state.holds[root];
            if self.facts.mutable.contains(root) {
                if hold != Hold::Owned {
                    self.fail(|| format!("returns without the value of `mut` parameter `{name}`"));
                }
            } else if Some(root) != returned {
                match hold {
                    Hold::Owned => {
                        self.fail(|| format!("returns while `{name}` still holds a value"))
                    }
                    Hold::Maybe => {
                        self.fail(|| format!("returns while `{name}` may still hold a value"))
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "mir_ownership_check_tests.rs"]
mod tests;
