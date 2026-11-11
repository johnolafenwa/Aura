//! Proves that the scalar element accesses of a counted loop stay inside
//! their list, so the loop's copy for a qualifying view (see
//! `native_codegen_list_snapshots`) runs them with no bounds check.
//!
//! A loop such as `for i in range(len(xs)): ys[i] = a * xs[i] + ys[i]`
//! reaches MIR as a header that branches on `i < end`, an advance
//! `i = i + 1`, and element reads and writes at `i`. The index is in range
//! on every iteration, but nothing in one block says so, and each access
//! keeps its bounds check and its call into the runtime for an index out
//! of range. LLVM does not vectorize a loop with such an exit.
//!
//! This pass finds, in the blocks of a loop that reads a list's view once:
//!
//! - **Counters.** A local `int64` that the loop only advances by a
//!   positive constant or sets to a constant, compared against a bound in a
//!   block that branches on `counter < bound`. On the branch's true edge
//!   the counter is in `[least, bound)`, where `least` is the smaller of
//!   its value when the loop is entered and the constants it is set to.
//! - **Bounds.** A local the loop does not assign, or an integer constant,
//!   or a local the loop assigns only the length of the list itself.
//! - **Places in range.** The counter on that edge, a copy of such a
//!   place, and such a place plus or minus a constant, until an
//!   instruction assigns the place. A place is known at the start of a
//!   block when it is known, at the same distance from the same counter,
//!   at the end of every block that jumps there.
//!
//! An access at a place `distance` from a counter is in range on every
//! iteration when `least + distance >= 0` and `bound + distance <=
//! len(list)`. The loop does not change either side, so native code checks
//! both once, where the loop is entered, together with the view: the
//! loop's copy runs only when they hold. When they do not, the loop runs
//! as it always has, and an index out of range stops the program at the
//! access that reaches it.
//!
//! The same guards bound the counter: it stays below `len(list)` less the
//! distance of a covered access, and a list holds fewer than 2^32
//! elements. So a checked sum or difference of a place in range and a
//! constant cannot overflow in the loop's copy, which computes it with no
//! overflow check. That covers the counter's own advance.

use std::collections::{HashMap, HashSet};

use super::list_snapshots::ListSnapshotRegion;
use crate::ast::BinaryOp;
use crate::mir::ownership::{instruction_accesses, Access};
use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, ArithmeticTrap, BasicBlock, CallTarget,
    Instruction, MirFunction, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{ListOp, RuntimeOp, ScalarElement};

/// The greatest distance from a counter an access is followed to.
const MAX_DISTANCE: i64 = 1 << 20;

/// What a counter is compared against.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) enum RangeBound {
    /// A local the loop does not assign.
    Place(String),
    /// An integer constant.
    Int(i64),
}

/// A condition native code checks where the loop is entered.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) enum RangeGuard {
    /// The local holds at least `least`.
    AtLeast { place: String, least: i64 },
    /// `bound + distance` is at most the list's length.
    Within { bound: RangeBound, distance: i64 },
}

/// The accesses of one list in one loop that stay in range while the
/// guards hold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RangeProof {
    /// The scalar kind of the list's elements, which every covered access
    /// names.
    pub(super) element: ScalarElement,
    pub(super) guards: Vec<RangeGuard>,
    /// Each covered access: its block and its position in the block.
    pub(super) covered: HashSet<(String, usize)>,
    /// Each checked `int64` sum or difference that cannot overflow: its
    /// block and its position in the block.
    pub(super) exact: HashSet<(String, usize)>,
}

fn root(place: &str) -> &str {
    place.split(['.', '[']).next().unwrap_or(place)
}

/// The local an instruction assigns, when it is an assignment to one.
fn assigned(instruction: &Instruction) -> Option<(&str, &Rvalue)> {
    match instruction {
        Instruction::Assign { target, value } => Some((target.as_str(), value)),
        _ => None,
    }
}

/// Whether `instruction` may change the local `name`.
fn changes(instruction: &Instruction, name: &str) -> bool {
    if matches!(assigned(instruction), Some((target, _)) if root(target) == name) {
        return true;
    }
    instruction_accesses(instruction)
        .into_iter()
        .any(|(place, access)| {
            root(place) == name && !matches!(access, Access::Read | Access::Consume)
        })
}

/// Whether anything but plain reads and assignments reaches the local
/// `name`: a loan, a closure or a task that captures it, a cleanup, or a
/// call that writes it back.
fn escapes(function: &MirFunction, name: &str) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            let mut mentions = Vec::new();
            instruction_place_mentions(instruction, &mut mentions);
            if !mentions.into_iter().any(|place| root(place) == name) {
                return false;
            }
            match instruction {
                Instruction::BeginLoan { .. }
                | Instruction::BeginReturnedLoan { .. }
                | Instruction::Reborrow { .. }
                | Instruction::ReborrowElement { .. }
                | Instruction::PushCleanup { .. } => true,
                Instruction::Assign { value, .. } => match value {
                    Rvalue::Closure { .. } | Rvalue::StartTask { .. } => true,
                    Rvalue::Call { args, .. } => args.iter().any(|arg| {
                        arg.writeback_place
                            .as_deref()
                            .is_some_and(|place| root(place) == name)
                    }),
                    _ => false,
                },
                Instruction::IndexInRange { .. } => false,
                _ => !instruction_accesses(instruction)
                    .into_iter()
                    .any(|(place, _)| root(place) == name),
            }
        })
}

/// Whether the terminator of a block may change the local `name`: any
/// terminator but a jump, a branch, or a return that names it.
fn terminator_changes(terminator: &Terminator, name: &str) -> bool {
    !matches!(
        terminator,
        Terminator::Goto(_) | Terminator::Branch { .. } | Terminator::Return(_)
    ) && terminator_place_mentions(terminator)
        .into_iter()
        .any(|place| root(place) == name)
}

fn int_constant(operand: &Operand) -> Option<i64> {
    match operand {
        Operand::Int(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

/// How far back through a block's temporaries a value is followed.
const MAX_CHAIN: u8 = 8;

/// A value as a local plus a constant, or as a constant alone.
type Affine<'f> = (Option<&'f str>, i64);

/// The value of `operand` just before instruction `position` of `block`,
/// as a local's value at that point plus a constant. MIR computes through
/// temporaries, so `i + 1` reaches an access as `%a = i`, `%b = 1`,
/// `%c = %a + %b`: this follows them back to `i`.
fn affine<'f>(
    block: &'f BasicBlock,
    position: usize,
    operand: &'f Operand,
    depth: u8,
) -> Option<Affine<'f>> {
    let place = match operand {
        Operand::Int(_) => return Some((None, int_constant(operand)?)),
        Operand::Place(place) if !place.contains(['.', '[']) => place.as_str(),
        _ => return None,
    };
    let own = Some((Some(place), 0));
    let Some(written) = block.instructions[..position]
        .iter()
        .rposition(|instruction| changes(instruction, place))
    else {
        return own;
    };
    if depth == 0 {
        return own;
    }
    let Some((_, value)) =
        assigned(&block.instructions[written]).filter(|(target, _)| *target == place)
    else {
        return own;
    };
    match affine_value(block, written, value, depth - 1) {
        Some((None, constant)) => Some((None, constant)),
        // The local the value came from still holds what it held then.
        Some((Some(base), constant))
            if !block.instructions[written..position]
                .iter()
                .any(|instruction| changes(instruction, base)) =>
        {
            Some((Some(base), constant))
        }
        _ => own,
    }
}

/// The value instruction `position` of `block` computes with `value`, when
/// it is a copy or an exact integer sum or difference.
fn affine_value<'f>(
    block: &'f BasicBlock,
    position: usize,
    value: &'f Rvalue,
    depth: u8,
) -> Option<Affine<'f>> {
    match value {
        Rvalue::Use(operand) => affine(block, position, operand, depth),
        Rvalue::Binary {
            op: op @ (BinaryOp::Add | BinaryOp::Sub),
            left,
            right,
            trap: ArithmeticTrap::None | ArithmeticTrap::Overflow,
            ..
        } => {
            let (left_base, left) = affine(block, position, left, depth)?;
            let (right_base, right) = affine(block, position, right, depth)?;
            match (op, left_base, right_base) {
                (BinaryOp::Add, base, None) | (BinaryOp::Add, None, base) => {
                    Some((base, left.checked_add(right)?))
                }
                (_, base, None) => Some((base, left.checked_sub(right)?)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The list an instruction measures, when it is `target = list.len()`.
fn length_of(value: &Rvalue) -> Option<&str> {
    match value {
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::List(ListOp::Len),
                    receiver: Some(Operand::Place(list)),
                    ..
                },
            ..
        } => Some(list.as_str()),
        _ => None,
    }
}

/// What a counter stays below.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Limit {
    /// A bound the loop does not change.
    Fixed(RangeBound),
    /// A local that, inside the loop, only ever takes the length of this
    /// list, directly or from the temporaries `sources`, which the loop
    /// only gives that length.
    LengthOf {
        place: String,
        sources: Vec<String>,
        list: String,
    },
}

/// A counter and the edge on which it is below its limit.
#[derive(Clone, Debug)]
struct Counter {
    name: String,
    /// The least constant the loop sets the counter to, when it sets it.
    least_reset: Option<i64>,
    limit: Limit,
    header: String,
    then_label: String,
}

/// The scalar element access `instruction` makes: the list, the index
/// place, and the element kind.
fn scalar_access(instruction: &Instruction) -> Option<(&str, &str, ScalarElement)> {
    let (_, value) = assigned(instruction)?;
    let Rvalue::Call {
        callee:
            CallTarget::Runtime {
                op: RuntimeOp::List(op),
                receiver: Some(Operand::Place(list)),
                ..
            },
        args,
        ..
    } = value
    else {
        return None;
    };
    let kind = match op {
        ListOp::IndexScalar(kind) | ListOp::SetIndexScalar(kind) => *kind,
        _ => return None,
    };
    match args.first().map(|arg| &arg.value) {
        Some(Operand::Place(index)) => Some((list.as_str(), index.as_str(), kind)),
        _ => None,
    }
}

/// The counters of the loop `region`. `is_index` says whether a local is
/// an `int64` native code keeps in one word.
fn counters(
    function: &MirFunction,
    region: &ListSnapshotRegion,
    is_index: &impl Fn(&str) -> bool,
) -> Vec<Counter> {
    let in_region = || {
        function
            .blocks
            .iter()
            .filter(|block| region.blocks.contains(&block.label))
    };
    let instructions = || in_region().flat_map(|block| &block.instructions);
    let usable = |name: &str| {
        is_index(name)
            && !escapes(function, name)
            && !in_region().any(|block| terminator_changes(&block.terminator, name))
    };
    let mut found = Vec::new();
    for block in in_region() {
        let Terminator::Branch {
            condition: Operand::Place(condition),
            then_label,
            ..
        } = &block.terminator
        else {
            continue;
        };
        if !region.blocks.contains(then_label) {
            continue;
        }
        // The compare that sets the condition, with nothing after it in
        // the block that changes either side.
        let Some(position) = block
            .instructions
            .iter()
            .rposition(|instruction| changes(instruction, condition))
        else {
            continue;
        };
        let Some((
            _,
            Rvalue::Binary {
                op: BinaryOp::Less,
                left,
                right,
                ..
            },
        )) = assigned(&block.instructions[position])
        else {
            continue;
        };
        let Some((Some(counter), 0)) = affine(block, position, left, MAX_CHAIN) else {
            continue;
        };
        if !usable(counter) || counter == condition {
            continue;
        }
        let later = &block.instructions[position + 1..];
        if later
            .iter()
            .any(|instruction| changes(instruction, counter))
        {
            continue;
        }
        // The loop only advances the counter or sets it to a constant.
        let mut least_reset = None;
        let steady = in_region().all(|block| {
            block
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, instruction)| changes(instruction, counter))
                .all(|(position, instruction)| {
                    let Some((_, value)) =
                        assigned(instruction).filter(|(target, _)| *target == counter)
                    else {
                        return false;
                    };
                    match affine_value(block, position, value, MAX_CHAIN) {
                        Some((None, constant)) => {
                            least_reset = Some(
                                least_reset.map_or(constant, |least: i64| least.min(constant)),
                            );
                            true
                        }
                        Some((Some(base), step)) => base == counter && step > 0,
                        None => false,
                    }
                })
        });
        if !steady {
            continue;
        }
        let limit = match affine(block, position, right, MAX_CHAIN) {
            Some((None, constant)) => Limit::Fixed(RangeBound::Int(constant)),
            Some((Some(bound), 0)) if bound != counter && usable(bound) => {
                if later.iter().any(|instruction| changes(instruction, bound)) {
                    continue;
                }
                let mut writers = instructions().filter(|instruction| changes(instruction, bound));
                match writers.next() {
                    None => Limit::Fixed(RangeBound::Place(bound.to_string())),
                    Some(first) => {
                        // Every assignment in the loop gives the bound the
                        // length of one list, directly or through a
                        // temporary the loop only gives that length.
                        let mut sources = Vec::new();
                        let mut length_source = |instruction: &'_ Instruction| -> Option<String> {
                            let (target, value) = assigned(instruction)?;
                            if target != bound {
                                return None;
                            }
                            if let Some(list) = length_of(value) {
                                return Some(list.to_string());
                            }
                            let Rvalue::Use(Operand::Place(source)) = value else {
                                return None;
                            };
                            if !usable(source) {
                                return None;
                            }
                            let mut lists = instructions()
                                .filter(|instruction| changes(instruction, source))
                                .map(|instruction| {
                                    assigned(instruction)
                                        .filter(|(target, _)| target == source)
                                        .and_then(|(_, value)| length_of(value))
                                });
                            let list = lists.next()??;
                            if !lists.all(|other| other == Some(list)) {
                                return None;
                            }
                            if !sources.contains(source) {
                                sources.push(source.clone());
                            }
                            Some(list.to_string())
                        };
                        let Some(list) = length_source(first) else {
                            continue;
                        };
                        if !writers.all(|writer| length_source(writer).as_ref() == Some(&list)) {
                            continue;
                        }
                        Limit::LengthOf {
                            place: bound.to_string(),
                            sources,
                            list,
                        }
                    }
                }
            }
            _ => continue,
        };
        found.push(Counter {
            name: counter.to_string(),
            least_reset,
            limit,
            header: block.label.clone(),
            then_label: then_label.clone(),
        });
    }
    found
}

/// The places known to be in range: each with the counter it follows and
/// its distance from it.
type Known<'f> = HashMap<&'f str, (usize, i64)>;

/// Applies `instruction` to the places in range.
fn step<'f>(
    known: &mut Known<'f>,
    block: &'f BasicBlock,
    position: usize,
    usable: &impl Fn(&str) -> bool,
) {
    let instruction = &block.instructions[position];
    let derived = assigned(instruction).and_then(|(target, value)| {
        if target.contains(['.', '[']) || !usable(target) {
            return None;
        }
        let (source, distance) = affine_value(block, position, value, MAX_CHAIN)?;
        let (counter, base) = *known.get(source?)?;
        let distance = base.checked_add(distance)?;
        (distance.abs() <= MAX_DISTANCE).then_some((target, (counter, distance)))
    });
    known.retain(|place, _| !changes(instruction, place));
    if let Some((target, fact)) = derived {
        known.insert(target, fact);
    }
}

/// The accesses of `region`'s list that stay in range, with the guards
/// that make them so, or `None` when the loop has none.
pub(super) fn range_proof(
    function: &MirFunction,
    region: &ListSnapshotRegion,
    is_index: impl Fn(&str) -> bool,
) -> Option<RangeProof> {
    let counters = counters(function, region, &is_index);
    if counters.is_empty() {
        return None;
    }
    let checked = std::cell::RefCell::new(HashMap::<String, bool>::new());
    let usable = |name: &str| {
        if let Some(&known) = checked.borrow().get(name) {
            return known;
        }
        let usable = is_index(name) && !escapes(function, name);
        checked.borrow_mut().insert(name.to_string(), usable);
        usable
    };
    let blocks = function
        .blocks
        .iter()
        .filter(|block| region.blocks.contains(&block.label))
        .collect::<Vec<_>>();
    let successors = |label: &str| -> Vec<&str> {
        blocks
            .iter()
            .find(|block| block.label == label)
            .map(|block| super::direct_terminator_successors(&block.terminator))
            .unwrap_or_default()
    };
    // What is known on the edge from `label` to `next`, given what is
    // known at the end of `label`.
    fn leave<'a>(
        counters: &'a [Counter],
        successors: &[&str],
        label: &str,
        next: &str,
        mut known: Known<'a>,
    ) -> Known<'a> {
        for (index, counter) in counters.iter().enumerate() {
            // A header whose two edges reach one block proves nothing.
            if counter.header == label
                && counter.then_label == next
                && successors.iter().filter(|other| **other == next).count() == 1
            {
                known.insert(counter.name.as_str(), (index, 0));
            }
        }
        known
    }
    // Each block's places in range at its start: `None` until an edge
    // reaches it. A block the loop is entered at starts with none.
    let mut at_start = HashMap::<&str, Option<Known<'_>>>::new();
    for block in &blocks {
        let entered = region.entries.iter().any(|(_, to)| *to == block.label);
        at_start.insert(block.label.as_str(), entered.then(Known::new));
    }
    let mut pending = blocks
        .iter()
        .map(|block| block.label.as_str())
        .collect::<Vec<_>>();
    while let Some(label) = pending.pop() {
        let Some(Some(start)) = at_start.get(label).cloned() else {
            continue;
        };
        let block = blocks
            .iter()
            .find(|block| block.label == label)
            .expect("a pending block is in the loop");
        let mut known = start;
        for position in 0..block.instructions.len() {
            step(&mut known, block, position, &usable);
        }
        known.retain(|place, _| !terminator_changes(&block.terminator, place));
        for next in successors(label) {
            let Some(slot) = at_start.get_mut(next) else {
                continue;
            };
            let arriving = leave(&counters, &successors(label), label, next, known.clone());
            let merged = match slot.as_ref() {
                None => arriving,
                Some(existing) => existing
                    .iter()
                    .filter(|(place, fact)| arriving.get(*place) == Some(fact))
                    .map(|(place, fact)| (*place, *fact))
                    .collect(),
            };
            if slot.as_ref() != Some(&merged) {
                *slot = Some(merged);
                if !pending.contains(&next) {
                    pending.push(next);
                }
            }
        }
    }

    let mut element = None;
    let mut guards = Vec::new();
    let mut covered = HashSet::new();
    // Sums of a place in range and a constant, by counter.
    let mut sums = Vec::new();
    let mut bounded = HashSet::new();
    let guard = |guards: &mut Vec<RangeGuard>, new: RangeGuard| {
        if !guards.contains(&new) {
            guards.push(new);
        }
    };
    for block in &blocks {
        let Some(Some(start)) = at_start.get(block.label.as_str()) else {
            continue;
        };
        let mut known = start.clone();
        for (position, instruction) in block.instructions.iter().enumerate() {
            if let Some((list, index, kind)) = scalar_access(instruction) {
                if let (true, Some(&(counter, distance))) = (list == region.list, known.get(index))
                {
                    let counter = &counters[counter];
                    // The counter is never below the constants it is set
                    // to, and the guard covers its value at the entry.
                    let above_zero = counter
                        .least_reset
                        .is_none_or(|least| least.saturating_add(distance) >= 0);
                    let within = match &counter.limit {
                        Limit::Fixed(_) => true,
                        Limit::LengthOf { list: measured, .. } => measured == list && distance <= 0,
                    };
                    if above_zero && within && element.is_none_or(|known| known == kind) {
                        element = Some(kind);
                        covered.insert((block.label.clone(), position));
                        bounded.insert(counter.name.as_str());
                        guard(
                            &mut guards,
                            RangeGuard::AtLeast {
                                place: counter.name.clone(),
                                least: -distance,
                            },
                        );
                        match &counter.limit {
                            Limit::Fixed(bound) => guard(
                                &mut guards,
                                RangeGuard::Within {
                                    bound: bound.clone(),
                                    distance,
                                },
                            ),
                            // The bound and its temporaries hold a length
                            // of the list once the loop assigns them.
                            // Until then they hold what they held at the
                            // entry, which the guards cover.
                            Limit::LengthOf { place, sources, .. } => {
                                for place in std::iter::once(place).chain(sources) {
                                    guard(
                                        &mut guards,
                                        RangeGuard::Within {
                                            bound: RangeBound::Place(place.clone()),
                                            distance: 0,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if let Some((
                _,
                value @ Rvalue::Binary {
                    trap: ArithmeticTrap::Overflow,
                    ..
                },
            )) = assigned(instruction)
            {
                let sum = affine_value(block, position, value, MAX_CHAIN)
                    .and_then(|(source, distance)| Some((known.get(source?)?, distance)))
                    .and_then(|((counter, base), distance)| {
                        Some((*counter, base.checked_add(distance)?))
                    });
                if let Some((counter, distance)) = sum {
                    if distance.abs() <= MAX_DISTANCE {
                        sums.push((counter, block.label.clone(), position));
                    }
                }
            }
            step(&mut known, block, position, &usable);
        }
    }
    let exact = sums
        .into_iter()
        .filter(|(counter, _, _)| bounded.contains(counters[*counter].name.as_str()))
        .map(|(_, label, position)| (label, position))
        .collect();
    Some(RangeProof {
        element: element?,
        guards,
        covered,
        exact,
    })
}

#[cfg(test)]
#[path = "native_codegen_range_loops_tests.rs"]
mod tests;
