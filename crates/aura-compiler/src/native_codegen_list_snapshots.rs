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
//! - A read (`len`, an index read of an element) only reads the list. Its
//!   runtime call may take in elements generated code appended, which sets
//!   the list's room, but leaves the view's access code, element address,
//!   and length as they were. An index read of a box lends the box where
//!   the list keeps it, or copies the element. Only the runtime lend moves
//!   elements into boxes of their own, and a list already in boxes reaches
//!   it only when the bound check fails, which stops the program.
//! - Any other operation here (an element loan, a store of a scalar) may
//!   write the list through the runtime, which refreshes the view. A
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
//! An `Array` local is read once the same way (ADR-0091). Its loop holds
//! only element reads and writes by integer coordinates, which leave the
//! Array's layout as it is. The loop's copy runs when the view names the
//! dtype and rank the accesses expect and, for a loop that writes, when
//! the Array is the only holder of its buffer. The copy then knows the
//! access code, the elements address, each extent, and that every write
//! lands in place.
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
use crate::runtime_ops::{ArrayOp, ListOp, RuntimeOp, ScalarElement};

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
    /// The temporaries the loop lends the list to, when the list is a
    /// field of an instance (see `field_list_snapshot_regions`).
    pub(super) temps: HashSet<String>,
    /// Set for the whole body of a method that keeps its receiver, `list`
    /// (see `method_region`), which is entered where the body begins.
    pub(super) method: bool,
    /// The blocks of an earlier loop on the list whose view words still
    /// hold at every entry of this one (see `carried_view`).
    pub(super) carry: Option<HashSet<String>>,
}

pub(super) fn root(place: &str) -> &str {
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
/// a loop may hold, or `None`.
fn view_use(instruction: &Instruction, list: &str) -> Option<ViewUse> {
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
                ListOp::Len
                | ListOp::IndexScalar(_)
                | ListOp::IndexScalarUnchecked(_)
                | ListOp::Index
                | ListOp::IndexUnchecked => Some(ViewUse::Reads),
                // A store into unshared storage of the element's own kind
                // keeps the storage, its form, and its length, and so does
                // an exchange of two elements.
                ListOp::SetIndexScalar(_) | ListOp::Swap => Some(ViewUse::Writes),
                _ => None,
            }
        }
        // An element read or write of an `Array` by integer coordinates
        // keeps the Array's layout. A write keeps the buffer too while
        // the Array is its only holder, which the loop's entry checks.
        Instruction::Assign {
            target,
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::Array(op),
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
                ArrayOp::ReadAt => Some(ViewUse::Reads),
                ArrayOp::WriteAt => Some(ViewUse::Writes),
                // A read by one coordinate, or by a tuple of them, which
                // `array_region_rank` turns away.
                ArrayOp::Index => Some(ViewUse::Reads),
                // The element count is a runtime call that changes nothing.
                ArrayOp::Len => Some(ViewUse::Reads),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The number of coordinates every element access of the Array `list` in
/// `region` names, when they all name the same number. `is_word` says
/// whether a local is one `int64`.
pub(super) fn array_region_rank(
    function: &MirFunction,
    region: &ListSnapshotRegion,
    is_word: impl Fn(&str) -> bool,
) -> Option<usize> {
    let mut rank = None;
    for block in &function.blocks {
        if !region.blocks.contains(&block.label) {
            continue;
        }
        for instruction in &block.instructions {
            let Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op: RuntimeOp::Array(op),
                                receiver: Some(Operand::Place(receiver)),
                                ..
                            },
                        args,
                        ..
                    },
                ..
            } = instruction
            else {
                continue;
            };
            if *receiver != region.list {
                continue;
            }
            let found = match (op, args.first().map(|arg| &arg.value)) {
                (ArrayOp::Len, _) => continue,
                (ArrayOp::ReadAt | ArrayOp::WriteAt, Some(Operand::Int(rank))) => *rank as usize,
                (ArrayOp::Index, Some(Operand::Int(_))) => 1,
                (ArrayOp::Index, Some(Operand::Place(place) | Operand::MovePlace(place)))
                    if is_word(place) =>
                {
                    1
                }
                _ => return None,
            };
            if rank.is_some_and(|rank| rank != found) {
                return None;
            }
            rank = Some(found);
        }
    }
    rank
}

/// The kind of scalar the element reads and writes of `region`'s list
/// name, when the loop has such accesses. A list has one element type, so
/// they all name one kind, and each checks the view for that kind's access
/// code.
pub(super) fn region_scalar_element(
    function: &MirFunction,
    region: &ListSnapshotRegion,
) -> Option<ScalarElement> {
    function
        .blocks
        .iter()
        .filter(|block| region.blocks.contains(&block.label))
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op:
                                    RuntimeOp::List(
                                        ListOp::IndexScalar(kind)
                                        | ListOp::IndexScalarUnchecked(kind)
                                        | ListOp::SetIndexScalar(kind),
                                    ),
                                receiver: Some(Operand::Place(receiver)),
                                ..
                            },
                        ..
                    },
                ..
            } if *receiver == region.list => Some(*kind),
            _ => None,
        })
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
/// says which locals hold a list native code may read this way.
pub(super) fn list_snapshot_regions(
    function: &MirFunction,
    candidate: impl Fn(&str) -> bool,
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
                                    op: RuntimeOp::List(_) | RuntimeOp::Array(_),
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
                match view_use(instruction, &list) {
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
        let first = regions.len();
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
                temps: HashSet::new(),
                method: false,
                carry: None,
            });
        }
        // A loop entered only after another loop on the list, through
        // blocks that keep the view, may take the words that loop read.
        for later in first..regions.len() {
            regions[later].carry = (first..regions.len())
                .find(|earlier| {
                    *earlier != later
                        && carried_view(
                            function,
                            &regions[*earlier],
                            &regions[later],
                            &clean,
                            &successors,
                        )
                })
                .map(|earlier| regions[earlier].blocks.clone());
        }
    }
    regions
}

/// Whether the view words read where `earlier` is entered still hold at
/// every entry of `later`: every path to one passes an entry of `earlier`
/// and, after the last, only blocks that keep the view. A block outside
/// `earlier` that may write the list keeps it only when `earlier` found
/// the storage unshared, which it checks when it writes.
fn carried_view(
    function: &MirFunction,
    earlier: &ListSnapshotRegion,
    later: &ListSnapshotRegion,
    clean: &HashMap<&str, (bool, bool)>,
    successors: &HashMap<&str, Vec<&str>>,
) -> bool {
    let keeps = |label: &str| {
        earlier.blocks.contains(label)
            || clean
                .get(label)
                .is_some_and(|(writes, _)| !writes || earlier.writes)
    };
    // Whether the words hold where each block ends, lowered from the most
    // hopeful guess until every edge agrees.
    let mut holds = function
        .blocks
        .iter()
        .map(|block| {
            let label = block.label.as_str();
            (label, label != function.entry && keeps(label))
        })
        .collect::<HashMap<_, _>>();
    let mut changed = true;
    while changed {
        changed = false;
        for (from, nexts) in successors {
            for next in nexts {
                let entered = earlier
                    .entries
                    .iter()
                    .any(|(source, target)| source.as_str() == *from && target.as_str() == *next);
                if holds[next] && !holds[from] && !entered {
                    holds.insert(*next, false);
                    changed = true;
                }
            }
        }
    }
    later.entries.iter().all(|(from, _)| holds[from.as_str()])
}

/// What `field_list_snapshot_regions` asks about a function.
pub(super) struct FieldListRules<'a> {
    /// The temporaries that take a field's box as it is (see
    /// `classes::field_lend_temps`).
    pub(super) lend_temps: &'a HashSet<String>,
    /// Whether the field place holds a list native code may read this way
    /// through the temporary a lend names.
    pub(super) candidate: &'a dyn Fn(&str, &str) -> bool,
    /// Whether the field place holds a scalar, which lies in the
    /// instance's block.
    pub(super) scalar_field: &'a dyn Fn(&str) -> bool,
    /// Whether a direct call of the function only reads scalar fields of
    /// an instance it is passed.
    pub(super) keeps_instances: &'a dyn Fn(&str) -> bool,
    /// The fields whose lists a call of the method may store elements
    /// into, when the method keeps its receiver (see `layout_keepers`).
    pub(super) keeps_receiver: &'a dyn Fn(&str) -> Option<&'a HashSet<String>>,
}

/// The fields whose lists `instruction` may store elements into, when it
/// is a call of a method that keeps its receiver (see `layout_keepers`)
/// on the instance `root_name`, which it names nowhere else.
fn receiver_keeper_call<'r>(
    instruction: &Instruction,
    root_name: &str,
    rules: &FieldListRules<'r>,
) -> Option<&'r HashSet<String>> {
    let Instruction::Assign {
        value:
            Rvalue::Call {
                callee:
                    CallTarget::Member {
                        object: Operand::Place(object),
                        receiver_place: Some(receiver),
                        dispatch: Some(crate::mir::MethodDispatch::Static { function }),
                        ..
                    },
                args,
                type_args,
            },
        ..
    } = instruction
    else {
        return None;
    };
    let mut mentions = Vec::new();
    instruction_place_mentions(instruction, &mut mentions);
    let passes = object == root_name
        && receiver == root_name
        && type_args.is_empty()
        && args.iter().all(|arg| arg.writeback_place.is_none())
        && mentions
            .into_iter()
            .filter(|place| root(place) == root_name)
            .all(|place| place == root_name);
    passes.then(|| (rules.keeps_receiver)(function)).flatten()
}

/// Whether `instruction` stores a scalar or plain record element into a
/// list, which keeps the list's storage, form, and length while nothing
/// else shares the storage.
fn stores_scalar(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::Call {
                callee: CallTarget::Runtime {
                    op: RuntimeOp::List(ListOp::SetIndexScalar(_)),
                    ..
                },
                ..
            },
            ..
        }
    )
}

/// The `mut self` methods of `functions` whose every call keeps the
/// receiver in its layout, each field's box in its field, and each field
/// list's storage, form, and length, with the fields whose lists each may
/// store elements into. `candidate` says which methods have a receiver
/// that keeps its fields in a block, `scalar_field` and `list_field` which
/// of a method's places are fields of its receiver that hold a scalar and
/// a list of scalars or plain records, and `keeps_instances` which
/// functions only read scalar fields of an instance they are passed.
///
/// Such a method names its receiver only to read or write scalar fields,
/// to lend a field's list to one operation that reads it or stores one of
/// its elements, to call a method of this kind on it, or to pass it to a
/// function that only reads scalar fields. It never appends to a list or
/// stores another list in a field, never hands the receiver to a closure,
/// a task, a loan, or a call that may write it back, and calls nothing
/// that does. A store into a list keeps its storage while nothing else
/// shares the storage, which a loop that reads the list's view once checks
/// where it is entered when a call may store into it.
pub(super) fn layout_keepers(
    functions: &[MirFunction],
    candidate: impl Fn(&MirFunction) -> bool,
    scalar_field: impl Fn(&MirFunction, &str) -> bool,
    list_field: impl Fn(&MirFunction, &str) -> bool,
    keeps_instances: &dyn Fn(&str) -> bool,
) -> HashMap<String, HashSet<String>> {
    const RECEIVER: &str = "self";
    // From every candidate, with no stores, down to the methods whose
    // bodies keep the receiver given the others.
    let mut keepers = functions
        .iter()
        .filter(|function| function.receiver.is_some() && candidate(function))
        .map(|function| (function.name.clone(), HashSet::new()))
        .collect::<HashMap<_, _>>();
    loop {
        let mut next = HashMap::new();
        for function in functions {
            if !keepers.contains_key(&function.name) {
                continue;
            }
            let lend_temps = super::classes::field_lend_temps(function, |_| false);
            let scalar = |place: &str| scalar_field(function, place);
            let rules = FieldListRules {
                lend_temps: &lend_temps,
                candidate: &|_, _| false,
                scalar_field: &scalar,
                keeps_instances,
                keeps_receiver: &|callee| keepers.get(callee),
            };
            if let Some(stores) = keeps_receiver_body(function, RECEIVER, &rules, &|place| {
                list_field(function, place)
            }) {
                next.insert(function.name.clone(), stores);
            }
        }
        if next == keepers {
            return keepers;
        }
        keepers = next;
    }
}

/// The fields whose lists the body of `function` may store elements into,
/// when it keeps its receiver `receiver` (see `layout_keepers`).
/// `list_field` says which places are fields of the receiver that hold
/// lists of scalars or plain records.
fn keeps_receiver_body(
    function: &MirFunction,
    receiver: &str,
    rules: &FieldListRules<'_>,
    list_field: &dyn Fn(&str) -> bool,
) -> Option<HashSet<String>> {
    let mut stores = HashSet::new();
    let mut keeps = true;
    for block in &function.blocks {
        let mut lent = None::<(&str, &str)>;
        for instruction in &block.instructions {
            if reaches_list(instruction, receiver)
                || matches!(instruction, Instruction::BeginElementLoan { source, .. } if root(source) == receiver)
            {
                return None;
            }
            match instruction {
                // A field of the receiver itself, as `self.items`.
                Instruction::Lend { place, temp, .. }
                    if list_field(place) && rules.lend_temps.contains(temp) =>
                {
                    lent = Some((temp, &place[receiver.len() + 1..]));
                }
                Instruction::EndLend { temp, .. } if lent.is_some_and(|(lent, _)| lent == temp) => {
                    lent = None;
                }
                _ if lent.is_some_and(|(temp, _)| mentioned_roots(instruction).contains(temp)) => {
                    let (temp, field) = lent.expect("checked above");
                    match view_use(instruction, temp) {
                        Some(ViewUse::Reads) => {}
                        Some(ViewUse::Writes) if stores_scalar(instruction) => {
                            stores.insert(field.to_string());
                        }
                        _ => return None,
                    }
                }
                _ if mentioned_roots(instruction).contains(receiver) => {
                    match receiver_keeper_call(instruction, receiver, rules) {
                        Some(called) => stores.extend(called.iter().cloned()),
                        None if keeps_field_list(instruction, receiver, "", rules)
                            && !matches!(
                                instruction,
                                Instruction::Lend { .. } | Instruction::EndLend { .. }
                            ) => {}
                        None => return None,
                    }
                }
                _ => {}
            }
        }
        // A branch on a scalar field, or a return of one, reads it where
        // it lies.
        keeps &= terminator_place_mentions(&block.terminator)
            .into_iter()
            .filter(|place| root(place) == receiver)
            .all(|place| (rules.scalar_field)(place));
    }
    keeps.then_some(stores)
}

/// Whether `instruction`, which names the instance `root`, leaves the
/// instance in its layout and the list in its field `list` as it is: it
/// reads or writes a scalar field in the block, lends another field's box
/// as it is, or passes the instance to a function that only reads scalar
/// fields.
fn keeps_field_list(
    instruction: &Instruction,
    root_name: &str,
    list: &str,
    rules: &FieldListRules<'_>,
) -> bool {
    let mut mentions = Vec::new();
    instruction_place_mentions(instruction, &mut mentions);
    mentions.extend(
        instruction_accesses(instruction)
            .into_iter()
            .map(|(place, _)| place),
    );
    let of_root = || {
        mentions
            .iter()
            .copied()
            .filter(|place| root(place) == root_name)
    };
    match instruction {
        Instruction::Lend { place, temp, .. } | Instruction::EndLend { place, temp, .. } => {
            place != list
                && !place.contains('[')
                && place.matches('.').count() == 1
                && rules.lend_temps.contains(temp)
        }
        Instruction::Assign {
            value: Rvalue::Use(_) | Rvalue::Binary { .. } | Rvalue::Unary { .. },
            ..
        } => of_root().all(|place| (rules.scalar_field)(place)),
        Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Name(function)
                        | CallTarget::Member {
                            dispatch: Some(crate::mir::MethodDispatch::Static { function }),
                            ..
                        },
                    args,
                    type_args,
                },
            ..
        } => {
            receiver_keeper_call(instruction, root_name, rules).is_some()
                || (rules.keeps_instances)(function)
                    && type_args.is_empty()
                    && args.iter().all(|arg| arg.writeback_place.is_none())
                    && of_root().all(|place| place == root_name || (rules.scalar_field)(place))
        }
        _ => false,
    }
}

/// The loops of `function` that read the view of a list in a field of an
/// instance once: `region.list` is the field place, as `self.items`.
///
/// Each access of such a list is a lend of the field's box to one list
/// operation. A loop qualifies when its every lend of the field is to an
/// operation that only reads the list, and every other instruction that
/// names the instance keeps its layout and the field (see
/// `keeps_field_list`). Nothing else reaches the instance while the loop
/// runs: `exclusive` says the function holds it alone, as a local or a
/// `mut` parameter, and no closure, task, or loan of this function names
/// it. So while the instance is in its typed layout and the field holds
/// its box alone when the loop is entered, both stay so, and the box's
/// view stays as the reads left it.
pub(super) fn field_list_snapshot_regions(
    function: &MirFunction,
    exclusive: impl Fn(&str) -> bool,
    rules: &FieldListRules<'_>,
) -> Vec<ListSnapshotRegion> {
    let mut lists = Vec::<&str>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Instruction::Lend { place, temp, .. } = instruction {
                if !place.contains('[')
                    && place.matches('.').count() == 1
                    && rules.lend_temps.contains(temp)
                    && exclusive(root(place))
                    && (rules.candidate)(place, temp)
                    && !lists.contains(&place.as_str())
                {
                    lists.push(place);
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
        let instance = root(list);
        // A loan of an element of the list, begun anywhere, may write the
        // list while the loop runs.
        let loans_element = |instruction: &Instruction| {
            matches!(instruction, Instruction::BeginElementLoan { source, .. }
                if source.strip_prefix(list).is_some_and(|rest| !rest.starts_with(char::is_alphanumeric)))
        };
        if function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| reaches_list(instruction, instance) || loans_element(instruction))
        {
            continue;
        }
        // The blocks that keep the instance and the list, with the
        // temporaries each lends the list to.
        let field = list.split_once('.').map_or("", |(_, field)| field);
        let mut clean = HashMap::<&str, (Vec<&str>, bool)>::new();
        'blocks: for block in &function.blocks {
            let mut temps = Vec::new();
            let mut writes = false;
            let mut lent = None::<&str>;
            for instruction in &block.instructions {
                match instruction {
                    Instruction::Lend { place, temp, .. }
                        if place == list && rules.lend_temps.contains(temp) =>
                    {
                        lent = Some(temp);
                        temps.push(temp.as_str());
                    }
                    Instruction::EndLend { place, temp, .. }
                        if place == list && lent == Some(temp.as_str()) =>
                    {
                        lent = None;
                    }
                    // The one operation a lent box goes to, and the facts
                    // about its index.
                    _ if lent.is_some_and(|temp| mentioned_roots(instruction).contains(temp)) => {
                        let temp = lent.expect("checked above");
                        match view_use(instruction, temp) {
                            Some(ViewUse::Reads) => {}
                            // A store of an element of the list's own kind.
                            Some(ViewUse::Writes) if stores_scalar(instruction) => writes = true,
                            _ => continue 'blocks,
                        }
                    }
                    _ if mentioned_roots(instruction).contains(instance)
                        && !keeps_field_list(instruction, instance, list, rules) =>
                    {
                        continue 'blocks;
                    }
                    _ if mentioned_roots(instruction).contains(instance) => {
                        writes |= receiver_keeper_call(instruction, instance, rules)
                            .is_some_and(|stores| stores.contains(field));
                    }
                    _ => {}
                }
            }
            if terminator_place_mentions(&block.terminator)
                .into_iter()
                .any(|place| root(place) == instance)
            {
                continue;
            }
            clean.insert(block.label.as_str(), (temps, writes));
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
            let temps = component
                .iter()
                .flat_map(|label| clean[label].0.iter().map(|temp| temp.to_string()))
                .collect::<HashSet<_>>();
            if temps.is_empty() {
                continue;
            }
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
                list: list.to_string(),
                blocks: component.iter().map(|label| label.to_string()).collect(),
                entries,
                writes: component.iter().any(|label| clean[label].1),
                temps,
                method: false,
                carry: None,
            });
        }
    }
    regions.extend(method_region(function, &exclusive, rules));
    regions
}

/// The whole body of a method that keeps its receiver: `self`, held
/// alone (see `field_list_snapshot_regions`), which every instruction
/// that names it keeps in its layout (see `keeps_field_list`). The
/// receiver then stays in the layout the body's entry found it in, and
/// the caller's write-through sink stays absent, for the whole call.
fn method_region(
    function: &MirFunction,
    exclusive: &impl Fn(&str) -> bool,
    rules: &FieldListRules<'_>,
) -> Option<ListSnapshotRegion> {
    const RECEIVER: &str = "self";
    if function.receiver.is_none() || !exclusive(RECEIVER) {
        return None;
    }
    let mut names = false;
    for block in &function.blocks {
        // A branch on a scalar field reads it where it lies.
        let terminator = terminator_place_mentions(&block.terminator)
            .into_iter()
            .filter(|place| root(place) == RECEIVER)
            .collect::<Vec<_>>();
        names |= !terminator.is_empty();
        let mut keeps = terminator
            .into_iter()
            .all(|place| (rules.scalar_field)(place));
        for instruction in &block.instructions {
            if mentioned_roots(instruction).contains(RECEIVER) {
                keeps &= keeps_field_list(instruction, RECEIVER, "", rules);
                names = true;
            }
        }
        if !keeps {
            return None;
        }
    }
    names.then(|| ListSnapshotRegion {
        list: RECEIVER.to_string(),
        blocks: function
            .blocks
            .iter()
            .map(|block| block.label.clone())
            .collect(),
        entries: Vec::new(),
        writes: false,
        temps: HashSet::new(),
        method: true,
        carry: None,
    })
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
    /// Each block that enters the loop, with the flag of the loop's lists
    /// of scalars alone, when the loop also reads other lists (see
    /// `version_snapshot_loops`).
    pub(super) scalar_entries: Vec<(Blk, Val)>,
    /// The access codes the loop's operations check the view for.
    pub(super) expected: Vec<i64>,
    /// The element accesses that stay in range while the guards hold
    /// (see `native_codegen_range_loops`).
    pub(super) range: Option<super::range_loops::RangeProof>,
    /// The kind of scalar the loop's element accesses name (see
    /// `region_scalar_element`). The loop's copy runs only for that
    /// kind's access code, which the copy then knows.
    pub(super) scalar: Option<ScalarElement>,
    /// The call slot of the mutable parameter that holds the list, whose
    /// write-through sink must be absent for the loop's copy to run.
    pub(super) sink: Option<usize>,
    /// Set when the local is an `Array`: what the loop's copy knows about
    /// its layout.
    pub(super) array: Option<ArraySnapshot>,
    /// Set when the list is a field of an instance: the variable that
    /// holds the field's box inside the loop.
    pub(super) field_box: Option<Var>,
}

/// The layout of an `Array` a loop reads once (ADR-0091). The loop's copy
/// runs only while the Array's view has `code`, and for a loop that writes
/// elements, while the Array holds its buffer alone.
#[derive(Clone, Debug)]
pub(super) struct ArraySnapshot {
    /// The view's access code, which names the dtype and `rank`.
    pub(super) code: i64,
    pub(super) rank: usize,
    /// The extent of each of the first `rank` axes.
    pub(super) extents: [Var; aura_runtime::native_runtime::DIRECT_ARRAY_MAX_RANK],
}

impl ListSnapshot {
    /// The kind of scalar whose access code the loop's copy runs for.
    fn scalar_element(&self) -> Option<ScalarElement> {
        self.range
            .as_ref()
            .map(|proof| proof.element)
            .or(self.scalar)
    }
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
            !snapshot.region.method
                && (snapshot.region.list == name || snapshot.region.temps.contains(name))
                && snapshot
                    .region
                    .blocks
                    .contains(&self.current_snapshot_label)
        })
    }

    /// The loop the block being compiled is in that reads the view of the
    /// list in the field `place` once.
    pub(super) fn field_snapshot(&self, place: &str) -> Option<usize> {
        self.list_snapshots.iter().position(|snapshot| {
            snapshot.field_box.is_some()
                && snapshot.region.list == place
                && snapshot
                    .region
                    .blocks
                    .contains(&self.current_snapshot_label)
        })
    }

    /// A loop the block being compiled is in that reads the view of a list
    /// in a field of the instance `instance` once. Its copy runs only
    /// while the instance is in its typed layout with no write-through
    /// sink.
    pub(super) fn instance_snapshot(&self, instance: &str) -> Option<usize> {
        self.list_snapshots.iter().position(|snapshot| {
            snapshot.field_box.is_some()
                && root(&snapshot.region.list) == instance
                && snapshot
                    .region
                    .blocks
                    .contains(&self.current_snapshot_label)
        })
    }

    /// `holds`, a check that passes in the copy of loop `index`.
    pub(super) fn holds_in_copy(&mut self, index: usize, holds: Val) -> Val {
        let holds = self.builder.uextend(types::I64, holds);
        let one = self.builder.iconst(types::I64, 1);
        let holds = self.keep_in_copy(index, one, holds);
        self.builder.icmp_imm(IntCC::NotEqual, holds, 0)
    }

    /// The box the field of snapshot `index` holds, or 0 when the instance
    /// is not in its typed layout, or the field's box is absent or has
    /// another holder.
    fn emit_field_box(&mut self, index: usize) -> std::result::Result<Val, String> {
        let place = self.list_snapshots[index].region.list.clone();
        let zero = self.builder.iconst(types::I64, 0);
        // A class native code does not keep in place has no such box.
        let Some((instance, slot, _)) = self
            .class_place(&place)?
            .filter(|(_, slot, _)| matches!(slot.kind, super::classes::SlotKind::Handle))
        else {
            return Ok(zero);
        };
        let var = self.local_vars(&instance)?[0];
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let ownership = super::ownership_layout();
        let flags = MemFlags::trusted();
        let object = self.builder.use_var(var);
        let check_block = self.builder.create_block();
        let block_block = self.builder.create_block();
        let count_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.builder
            .brif(object, check_block, &[], join_block, &[zero]);

        self.builder.switch_to_block(check_block);
        self.builder.seal_block(check_block);
        let access = self.builder.uload32(flags, object, layout.access_offset);
        let typed = self.builder.icmp_imm(IntCC::Equal, access, slot.access);
        self.builder
            .brif(typed, block_block, &[], join_block, &[zero]);

        self.builder.switch_to_block(block_block);
        self.builder.seal_block(block_block);
        let block = self.class_block(object);
        let boxed = self.builder.load(types::I64, flags, block, slot.offset);
        self.builder
            .brif(boxed, count_block, &[], join_block, &[zero]);

        self.builder.switch_to_block(count_block);
        self.builder.seal_block(count_block);
        // The end of each lend finds the box with one holder, as the
        // loop's copy takes it to be.
        let count = self
            .builder
            .uload32(flags, boxed, ownership.ref_count_offset);
        let alone = self.builder.icmp_imm(IntCC::Equal, count, 1);
        let held = self.builder.select(alone, boxed, zero);
        self.builder.jump(join_block, &[held]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(self.builder.block_params(join_block)[0])
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

    /// Reads, where the body of a method that keeps its receiver begins,
    /// whether the receiver is in its typed layout with no write-through
    /// sink, which the copy of the whole body runs for (see
    /// `method_region`). The MIR entry block's code then begins in a block
    /// of its own.
    pub(super) fn enter_method_snapshot(
        &mut self,
        function: &MirFunction,
    ) -> std::result::Result<(), String> {
        let Some(index) = self
            .list_snapshots
            .iter()
            .position(|snapshot| snapshot.region.method)
        else {
            return Ok(());
        };
        let start = self.blocks[&function.entry];
        self.builder.switch_to_block(start);
        // The layout every field of the receiver's class lies in.
        let mut access = None;
        for block in &function.blocks {
            for instruction in &block.instructions {
                let mut mentions = Vec::new();
                instruction_place_mentions(instruction, &mut mentions);
                for place in mentions {
                    if access.is_none() && root(place) == "self" {
                        access = self.class_place(place)?.map(|(_, slot, _)| slot.access);
                    }
                }
            }
        }
        // A class whose fields lie in a map has no layout to keep, and the
        // body no copy.
        let Some(access) = access else {
            return Ok(());
        };
        let zero = self.builder.iconst(types::I64, 0);
        let object = self.local_vars("self")?[0];
        let object = self.builder.use_var(object);
        let check_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.builder
            .brif(object, check_block, &[], join_block, &[zero]);
        self.builder.switch_to_block(check_block);
        self.builder.seal_block(check_block);
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let actual = self
            .builder
            .uload32(MemFlags::trusted(), object, layout.access_offset);
        let same = self.builder.icmp_imm(IntCC::Equal, actual, access);
        let same = self.builder.uextend(types::I64, same);
        self.builder.jump(join_block, &[same]);
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let mut flag = self.builder.block_params(join_block)[0];
        if let Some(slot) = self.list_snapshots[index].sink {
            let sink = self.current_mutable_sink(slot);
            let direct = self.builder.icmp_imm(IntCC::Equal, sink, 0);
            let direct = self.builder.uextend(types::I64, direct);
            flag = self.builder.band(flag, direct);
        }
        self.builder.def_var(self.list_snapshots[index].ok, flag);
        let pad = self.builder.current_block().unwrap_or(start);
        let body = self.builder.create_block();
        self.builder.jump(body, &[]);
        self.builder.switch_to_block(body);
        self.builder.seal_block(body);
        self.blocks.insert(function.entry.clone(), body);
        self.list_snapshots[index].entries.push((pad, flag, flag));
        Ok(())
    }

    /// Fills the blocks the terminator just compiled jumps through to
    /// enter loops: each reads the views of its loops' lists and jumps on.
    pub(super) fn fill_snapshot_pads(&mut self) -> std::result::Result<(), String> {
        for (pad, target, entering) in std::mem::take(&mut self.pending_snapshot_pads) {
            self.builder.switch_to_block(pad);
            self.builder.seal_block(pad);
            let mut oks = Vec::new();
            let sources = entering
                .iter()
                .map(|index| self.carry_source(*index))
                .collect::<Option<Vec<_>>>();
            match sources {
                // Loops entered after other loops on each of their lists,
                // whose words still hold, take them while every one of
                // those loops found its view qualified.
                Some(sources) => {
                    let mut all = None;
                    for source in &sources {
                        let ok = self.builder.use_var(self.list_snapshots[*source].ok);
                        all = Some(match all {
                            None => ok,
                            Some(all) => self.builder.band(all, ok),
                        });
                    }
                    let all = all.expect("a pad enters at least one loop");
                    let carry_block = self.builder.create_block();
                    let fresh_block = self.builder.create_block();
                    let join_block = self.builder.create_block();
                    for _ in 0..4 * entering.len() {
                        self.builder.append_block_param(join_block, types::I64);
                    }
                    self.builder.brif(all, carry_block, &[], fresh_block, &[]);
                    self.builder.switch_to_block(carry_block);
                    self.builder.seal_block(carry_block);
                    let mut words = Vec::new();
                    for (index, source) in entering.iter().zip(&sources) {
                        words.extend(self.emit_carried_snapshot(*index, *source)?);
                    }
                    self.builder.jump(join_block, &words);
                    self.builder.switch_to_block(fresh_block);
                    self.builder.seal_block(fresh_block);
                    words.clear();
                    for index in &entering {
                        self.emit_list_snapshot(*index)?;
                        let snapshot = &self.list_snapshots[*index];
                        let vars = [
                            snapshot.ok,
                            snapshot.access,
                            snapshot.len,
                            snapshot.elements,
                        ];
                        words.extend(vars.map(|variable| self.builder.use_var(variable)));
                    }
                    self.builder.jump(join_block, &words);
                    self.builder.switch_to_block(join_block);
                    self.builder.seal_block(join_block);
                    let params = self.builder.block_params(join_block).to_vec();
                    for (index, params) in entering.iter().zip(params.chunks(4)) {
                        let snapshot = &self.list_snapshots[*index];
                        let vars = [
                            snapshot.ok,
                            snapshot.access,
                            snapshot.len,
                            snapshot.elements,
                        ];
                        for (variable, value) in vars.into_iter().zip(params) {
                            self.builder.def_var(variable, *value);
                        }
                        oks.push((*index, (params[0], params[1])));
                    }
                }
                None => {
                    for index in &entering {
                        oks.push((*index, self.emit_list_snapshot(*index)?));
                    }
                }
            }
            // A loop that reads several lists' views once runs its copy
            // while every view qualifies. The jump on is in the block the
            // reads end in.
            let pad = self.builder.current_block().unwrap_or(pad);
            for (index, (_, access)) in &oks {
                let mut flag = None;
                let mut scalars = None;
                let mut mixed = false;
                for (other, (ok, _)) in &oks {
                    if self.list_snapshots[*other].region.blocks
                        == self.list_snapshots[*index].region.blocks
                    {
                        flag = Some(match flag {
                            None => *ok,
                            Some(flag) => self.builder.band(flag, *ok),
                        });
                        if self.list_snapshots[*other].scalar_element().is_some() {
                            scalars = Some(match scalars {
                                None => *ok,
                                Some(flag) => self.builder.band(flag, *ok),
                            });
                        } else {
                            mixed = true;
                        }
                    }
                }
                let flag = flag.expect("a loop's own flag is among them");
                self.list_snapshots[*index]
                    .entries
                    .push((pad, flag, *access));
                if let Some(scalars) = scalars.filter(|_| mixed) {
                    self.list_snapshots[*index]
                        .scalar_entries
                        .push((pad, scalars));
                }
            }
            self.builder.jump(target, &[]);
        }
        Ok(())
    }

    /// Copies the view of snapshot `index`'s Array into its variables,
    /// with whether the loop's copy may run: the view has the access code
    /// the loop's accesses expect, and a loop that writes elements finds
    /// the Array the only holder of its buffer. Nothing in such a loop
    /// names the Array but element accesses, so the layout, the buffer,
    /// and the holder count stay as they are while it runs.
    fn emit_array_snapshot(
        &mut self,
        index: usize,
        array: &ArraySnapshot,
    ) -> std::result::Result<(Val, Val), String> {
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
        let layout_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        // The flag, the access code, the elements, the length word, and
        // each extent.
        let words = 4 + array.extents.len();
        for _ in 0..words {
            self.builder.append_block_param(join_block, types::I64);
        }
        let zero = self.builder.iconst(types::I64, 0);
        let none = vec![zero; words];
        self.builder
            .brif(handle, read_block, &[], join_block, &none);

        self.builder.switch_to_block(read_block);
        self.builder.seal_block(read_block);
        let access = self.builder.uload32(flags, handle, layout.access_offset);
        let exact = self.builder.icmp_imm(IntCC::Equal, access, array.code);
        let mut other = none.clone();
        other[1] = access;
        self.builder
            .brif(exact, layout_block, &[], join_block, &other);

        self.builder.switch_to_block(layout_block);
        self.builder.seal_block(layout_block);
        let elements = self
            .builder
            .load(types::I64, flags, handle, layout.elements_offset);
        // The length word is the length of a rank-1 Array and the address
        // of the extents otherwise.
        let length = self
            .builder
            .load(types::I64, flags, handle, layout.len_offset);
        let mut extents = vec![zero; array.extents.len()];
        for (axis, extent) in extents.iter_mut().enumerate().take(array.rank) {
            *extent = match array.rank {
                1 => length,
                _ => self
                    .builder
                    .load(types::I64, flags, length, axis as i32 * 8),
            };
        }
        let mut qualifies = self.builder.iconst(types::I64, 1);
        if snapshot.region.writes {
            let holders = self
                .builder
                .load(types::I64, flags, handle, layout.holders_offset);
            let count = self.builder.load(types::I64, flags, holders, 0);
            let alone = self.builder.icmp_imm(IntCC::Equal, count, 1);
            let alone = self.builder.uextend(types::I64, alone);
            qualifies = self.builder.band(qualifies, alone);
        }
        if let Some(slot) = snapshot.sink {
            let sink = self.current_mutable_sink(slot);
            let direct = self.builder.icmp_imm(IntCC::Equal, sink, 0);
            let direct = self.builder.uextend(types::I64, direct);
            qualifies = self.builder.band(qualifies, direct);
        }
        let mut found = vec![qualifies, access, elements, length];
        found.extend(extents);
        self.builder.jump(join_block, &found);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let params = self.builder.block_params(join_block).to_vec();
        for (variable, value) in [
            snapshot.ok,
            snapshot.access,
            snapshot.elements,
            snapshot.len,
        ]
        .into_iter()
        .chain(array.extents)
        .zip(params.iter().copied())
        {
            self.builder.def_var(variable, value);
        }
        Ok((params[0], params[1]))
    }

    /// What the loop's copy knows about the Array `array`, when the access
    /// being compiled is in a loop that reads its layout once: the loop's
    /// index and the layout.
    pub(super) fn array_snapshot(&self, array: Val, rank: usize) -> Option<(usize, ArraySnapshot)> {
        let index = *self.snapshot_vals.get(&array)?;
        let snapshot = self.list_snapshots[index].array.clone()?;
        (snapshot.rank == rank).then_some((index, snapshot))
    }

    /// The elements address of loop `index`'s Array as its entry read it.
    pub(super) fn snapshot_elements(&mut self, index: usize) -> Val {
        self.builder.use_var(self.list_snapshots[index].elements)
    }

    /// The loop on the same list whose view words snapshot `index` takes
    /// (see `carried_view`), as an index into `list_snapshots`.
    fn carry_source(&self, index: usize) -> Option<usize> {
        let snapshot = &self.list_snapshots[index];
        let blocks = snapshot.region.carry.as_ref()?;
        self.list_snapshots.iter().position(|other| {
            other.region.list == snapshot.region.list
                && other.region.blocks == *blocks
                && other.array.is_none()
        })
    }

    /// The flag and the view words of snapshot `index`'s list, which loop
    /// `source` found qualified with the words they still are: the words
    /// are read again with no checks of their own.
    fn emit_carried_snapshot(
        &mut self,
        index: usize,
        source: usize,
    ) -> std::result::Result<[Val; 4], String> {
        let snapshot = self.list_snapshots[index].clone();
        let source = self.list_snapshots[source].clone();
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let source_ok = self.builder.use_var(source.ok);
        let handle = self.local_list_handle(&snapshot.region.list)?;
        let access = self.builder.uload32(flags, handle, layout.access_offset);
        let len = self
            .builder
            .load(types::I64, flags, handle, layout.len_offset);
        let elements = self
            .builder
            .load(types::I64, flags, handle, layout.elements_offset);
        let qualifies =
            self.view_qualifies(&snapshot, handle, access, len, Some((&source, source_ok)))?;
        let ok = self.builder.uextend(types::I64, qualifies);
        Ok([ok, access, len, elements])
    }

    /// Copies the view of snapshot `index`'s list into its variables, with
    /// whether the view qualifies (see the module documentation).
    fn emit_list_snapshot(&mut self, index: usize) -> std::result::Result<(Val, Val), String> {
        let snapshot = self.list_snapshots[index].clone();
        if let Some(array) = &snapshot.array {
            return self.emit_array_snapshot(index, array);
        }
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let handle = match snapshot.field_box {
            Some(field_box) => {
                let boxed = self.emit_field_box(index)?;
                self.builder.def_var(field_box, boxed);
                boxed
            }
            None => self.local_list_handle(&snapshot.region.list)?,
        };
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
        let qualifies = self.view_qualifies(&snapshot, handle, access, len, None)?;
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

    /// Whether the view words `access` and `len` of the list `handle`
    /// let the loop of `snapshot` run its copy (see the module
    /// documentation). `carried` is the loop the words come from, with
    /// its flag, when they are that loop's.
    fn view_qualifies(
        &mut self,
        snapshot: &ListSnapshot,
        handle: Val,
        access: Val,
        len: Val,
        carried: Option<(&ListSnapshot, Val)>,
    ) -> std::result::Result<Val, String> {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let mut qualifies = match carried {
            // The words are the ones that loop found in its own form.
            Some((source, source_ok)) if source.scalar_element() == snapshot.scalar_element() => {
                self.builder.icmp_imm(IntCC::NotEqual, source_ok, 0)
            }
            _ => self.view_form(snapshot, access),
        };
        // A loop that wrote the list found its storage unshared, and the
        // words it carries hold only while nothing shares it.
        let carried_room = carried.is_some_and(|(source, _)| source.region.writes);
        if snapshot.region.writes && !carried_room {
            let room = self.builder.uload32(flags, handle, layout.capacity_offset);
            let has_room = self.builder.icmp_imm(IntCC::NotEqual, room, 0);
            qualifies = self.builder.band(qualifies, has_room);
        }
        // The other loop checked the same sink.
        let sink = match carried {
            Some((source, _)) if source.sink == snapshot.sink => None,
            _ => snapshot.sink,
        };
        if let Some(slot) = sink {
            // A caller that asked for each write to be handed back takes
            // a reference to the list with it, so the storage is shared.
            let sink = self.current_mutable_sink(slot);
            let direct = self.builder.icmp_imm(IntCC::Equal, sink, 0);
            qualifies = self.builder.band(qualifies, direct);
        }
        // The carried length is the one the other loop's guards found long
        // enough for each constant bound and distance.
        let carried_reach = carried
            .and_then(|(source, _)| source.range.as_ref())
            .and_then(|proof| {
                use super::range_loops::{RangeBound, RangeGuard};
                proof
                    .guards
                    .iter()
                    .filter_map(|guard| match guard {
                        RangeGuard::Within {
                            bound: RangeBound::Int(bound),
                            distance,
                        } => bound.checked_add(*distance),
                        _ => None,
                    })
                    .max()
            });
        if let Some(proof) = &snapshot.range {
            for guard in &proof.guards {
                use super::range_loops::{RangeBound, RangeGuard};
                if let (RangeGuard::Within { bound, distance }, Some(carried_reach)) =
                    (guard, carried_reach)
                {
                    let bound = match bound {
                        RangeBound::Int(value) => Some(*value),
                        RangeBound::Place(place) => {
                            let value = self.range_word(place)?;
                            self.builder.integer_constant(value)
                        }
                    };
                    if bound
                        .and_then(|bound| bound.checked_add(*distance))
                        .is_some_and(|reach| reach <= carried_reach)
                    {
                        continue;
                    }
                }
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
        Ok(qualifies)
    }

    /// Whether the access code `access` names a form the loop of
    /// `snapshot` reads its view once in.
    fn view_form(&mut self, snapshot: &ListSnapshot, access: Val) -> Val {
        use aura_runtime::native_runtime::{
            DirectListAccess, FIRST_CLASS_ACCESS, FIRST_PLAIN_ACCESS,
        };
        match snapshot.scalar_element() {
            // The loop's scalar accesses check for one access code, a
            // scalar form, and the copy runs for that one.
            Some(element) => {
                let (code, _) = super::scalar_list_slot(element);
                self.builder.icmp_imm(IntCC::Equal, access, code as i64)
            }
            None => {
                // A list of scalars other than `str` views, of boxes, or
                // of plain records.
                let scalar_form = self
                    .builder
                    .iadd_imm(access, -(DirectListAccess::Int64 as i64));
                let scalar = self.builder.icmp_imm(
                    IntCC::UnsignedLessThan,
                    scalar_form,
                    DirectListAccess::Float32 as i64 - DirectListAccess::Int64 as i64 + 1,
                );
                let not_str =
                    self.builder
                        .icmp_imm(IntCC::NotEqual, access, DirectListAccess::Str as i64);
                let scalar = self.builder.band(scalar, not_str);
                let boxes =
                    self.builder
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
                self.builder.bor(form, typed)
            }
        }
    }

    /// The box of the local list `list`.
    fn local_list_handle(&mut self, list: &str) -> std::result::Result<Val, String> {
        let vars = self.local_vars(list)?;
        let [var] = vars.as_slice() else {
            return Err(format!(
                "direct backend read the view of `{list}`, which is not one box"
            ));
        };
        Ok(self.builder.use_var(*var))
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
        let flags = MemFlags::trusted_in;
        let read = match word {
            ViewWord::Access => {
                self.builder
                    .uload32(flags(AliasClass::BoxAccess), list, layout.access_offset)
            }
            ViewWord::Len => self.builder.load(
                types::I64,
                flags(AliasClass::BoxLen),
                list,
                layout.len_offset,
            ),
            ViewWord::Elements => self.builder.load(
                types::I64,
                flags(AliasClass::BoxElements),
                list,
                layout.elements_offset,
            ),
            ViewWord::Room => {
                self.builder
                    .uload32(flags(AliasClass::BoxRoom), list, layout.capacity_offset)
            }
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
    /// qualifies. Loops inside others come first, so the copy of a loop
    /// around them, or across them, holds each of their versions.
    pub(super) fn version_snapshot_loops(&mut self) {
        // The loops with the same blocks, which one copy serves.
        let mut groups = Vec::<Vec<usize>>::new();
        for index in 0..self.list_snapshots.len() {
            let blocks = &self.list_snapshots[index].region.blocks;
            match groups
                .iter_mut()
                .find(|group| self.list_snapshots[group[0]].region.blocks == *blocks)
            {
                Some(group) => group.push(index),
                None => groups.push(vec![index]),
            }
        }
        groups.sort_by_key(|group| self.list_snapshots[group[0]].region.blocks.len());
        for group in groups {
            let index = group[0];
            // A loop over one list whose operations all check for one
            // access code runs its copy only for that code, which the copy
            // then knows. A loop with scalar element accesses checked the
            // code where it was entered.
            let code_of = |snapshot: &ListSnapshot| {
                snapshot
                    .scalar_element()
                    .map(|element| super::scalar_list_slot(element).0 as i64)
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
                let known = code_of(&self.list_snapshots[*member]).or(expected);
                blocks.extend(self.list_snapshots[*member].ir_blocks.iter().copied());
                selects.extend(
                    self.list_snapshots[*member]
                        .selects
                        .iter()
                        .map(|(select, access)| (*select, known.filter(|_| *access))),
                );
            }
            let entries = self.list_snapshots[index]
                .entries
                .iter()
                .map(|(pad, flag, access)| (*pad, *flag, expected.map(|code| (*access, code))))
                .collect::<Vec<_>>();
            // A loop that reads lists of scalars and other lists first gets
            // a copy for while the lists of scalars qualify, whose view is
            // the cheapest to keep and the likeliest to qualify. The copy
            // for while every list qualifies is then tried first where the
            // loop is entered, so a list that does not qualify costs the
            // others nothing.
            // A copy with nothing to settle gains nothing.
            if selects.is_empty() {
                continue;
            }
            if !self.list_snapshots[index].scalar_entries.is_empty() {
                let scalar_selects = group
                    .iter()
                    .filter(|member| self.list_snapshots[**member].scalar_element().is_some())
                    .flat_map(|member| {
                        let known = code_of(&self.list_snapshots[*member]);
                        self.list_snapshots[*member]
                            .selects
                            .iter()
                            .map(move |(select, access)| (*select, known.filter(|_| *access)))
                    })
                    .collect::<Vec<_>>();
                let scalar_entries = self.list_snapshots[index]
                    .scalar_entries
                    .iter()
                    .map(|(pad, flag)| (*pad, *flag, None))
                    .collect::<Vec<_>>();
                self.builder
                    .version_blocks(&blocks, &scalar_selects, &scalar_entries);
            }
            self.builder.version_blocks(&blocks, &selects, &entries);
            self.builder.seal_versions();
        }
    }
}

#[cfg(test)]
#[path = "native_codegen_list_snapshots_tests.rs"]
mod tests;
