//! Finds the `Lookup` and `QueueReceive` temporaries that native code
//! keeps as the payload's box alone.
//!
//! `match table.get(key)` lowers to a temporary that holds a boxed
//! `Lookup`, a match on its variant, and a copy of its payload. When the
//! temporary is used for nothing else, the code generator stores the
//! payload's box in it instead, or null for `Lookup.Missing`. The match
//! then tests the word for null, and the payload is a new reference to the
//! same box. No `Lookup` is built, tagged, matched by name, or copied.
//!
//! A queue receive (`queue.get()` or a `for` loop over a queue) is kept the
//! same way when its match tests only `QueueReceive.Item`: the slot holds
//! the item's box, or null for every other variant. A `put` whose result is
//! only discarded keeps null, and no `Result` is built.
//!
//! `match own str.from_bytes(bytes)` with an `Err` arm that binds nothing
//! keeps the text's box, or null when the bytes are not UTF-8. A slot may
//! move into another temporary, as `match own` does, and the other
//! temporary is then a slot too.

use std::collections::{HashMap, HashSet};

use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, BasicBlock, CallTarget, Instruction,
    MirArg, MirFunction, MirMatchArm, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{DictOp, HostBuiltin, ListOp, QueueOp, RuntimeOp};

/// Whether `value` looks a key up in a `dict` with `get`.
pub(super) fn is_dict_get(value: &Rvalue) -> bool {
    slot_kind(value) == Some(SlotKind::Lookup)
}

/// What a slot's temporary would otherwise hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotKind {
    /// A `Lookup` from a `dict` `get`.
    Lookup,
    /// A `QueueReceive` from a queue receive that waits for an item.
    Receive,
    /// The `Result` of a queue `put`, which nothing reads.
    Discarded,
    /// The `Result` of `str.from_bytes`, whose error nothing reads.
    Text,
}

impl SlotKind {
    /// Whether `arm` reads only what the slot keeps: whether there is a
    /// payload. `otherwise` is where the match goes when no arm matches.
    fn tests(self, arm: &MirMatchArm, otherwise: &str) -> bool {
        let enum_name = arm.enum_name.as_deref();
        let variant_name = arm.variant_name.as_deref();
        match self {
            SlotKind::Lookup => {
                matches!(enum_name, None | Some("Lookup"))
                    && matches!(variant_name, Some("Found" | "Missing"))
            }
            // A null slot stands for every variant but `Item`, so an arm
            // for another variant must go where the match does without it,
            // as a `for` loop's `Closed` and `Cancelled` arms do.
            SlotKind::Receive => {
                matches!(enum_name, None | Some("QueueReceive"))
                    && (variant_name == Some("Item")
                        || (matches!(variant_name, Some("Closed" | "TimedOut" | "Cancelled"))
                            && arm.label == otherwise))
            }
            SlotKind::Text => {
                matches!(enum_name, None | Some("Result"))
                    && matches!(variant_name, Some("Ok" | "Err"))
            }
            SlotKind::Discarded => false,
        }
    }

    /// Whether the slot keeps the payload a `VariantPayload` of
    /// `variant_name` reads.
    fn keeps_payload_of(self, variant_name: &str) -> bool {
        match self {
            SlotKind::Lookup | SlotKind::Receive => true,
            SlotKind::Text => variant_name == "Ok",
            SlotKind::Discarded => false,
        }
    }
}

fn slot_kind(value: &Rvalue) -> Option<SlotKind> {
    match value {
        Rvalue::Call {
            callee: CallTarget::Runtime { op, .. },
            ..
        } => match op {
            RuntimeOp::Dict(DictOp::Get | DictOp::GetStr | DictOp::GetInt64) => {
                Some(SlotKind::Lookup)
            }
            RuntimeOp::Queue(
                QueueOp::Get | QueueOp::GetInTaskGroup | QueueOp::GetWithRegisteredProducers,
            ) => Some(SlotKind::Receive),
            RuntimeOp::Queue(QueueOp::Put) => Some(SlotKind::Discarded),
            RuntimeOp::Host(builtin) if is_str_from_bytes(*builtin) => Some(SlotKind::Text),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `builtin` is `str.from_bytes`.
pub(super) fn is_str_from_bytes(builtin: HostBuiltin) -> bool {
    builtin.name() == "str.from_bytes"
}

/// The variant a slot holds a payload for: the one whose test is "not
/// null".
pub(super) fn is_payload_variant(variant_name: Option<&str>) -> bool {
    matches!(variant_name, Some("Found" | "Item" | "Ok"))
}

/// The temporary a slot moves out of, when `value` is that move.
fn moved_temporary(value: &Rvalue) -> Option<&str> {
    match value {
        Rvalue::Use(Operand::MovePlace(source)) if is_temporary(source) => Some(source),
        _ => None,
    }
}

fn is_temporary(place: &str) -> bool {
    place.starts_with("%t") && !place.contains('.')
}

/// The temporaries of `function` that may hold a lookup's or a receive's
/// payload in place of the `Lookup` or `QueueReceive`. Each one is a
/// compiler temporary that is assigned only from one kind of `get` and is
/// otherwise only matched on the variants its slot tells apart, unpacked
/// in one place at most once per lookup, and released.
pub(super) fn lookup_payload_slots(function: &MirFunction) -> HashSet<String> {
    let mut candidates = HashMap::<&str, Candidate>::new();
    let mut rejected = HashSet::<&str>::new();
    // Each move of one temporary into another, by target.
    let mut moves = Vec::<(&str, &str, &str)>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Instruction::Assign { target, value } = instruction {
                if let Some(source) = moved_temporary(value).filter(|_| is_temporary(target)) {
                    moves.push((target, source, &block.label));
                    continue;
                }
                let kind = slot_kind(value).filter(|_| is_temporary(target));
                if let Some(kind) = kind {
                    let candidate = candidates.entry(target).or_default();
                    if candidate
                        .kind
                        .replace(kind)
                        .is_some_and(|other| other != kind)
                    {
                        rejected.insert(target);
                    }
                    candidate.assigned.insert(&block.label);
                } else {
                    rejected.insert(target);
                }
            }
        }
    }
    for param in &function.params {
        rejected.insert(&param.name);
    }
    // A move from a slot makes its target a slot of the same kind. A move
    // from anything else, or into a temporary assigned another way, makes
    // neither one a slot.
    let mut grew = true;
    while grew {
        grew = false;
        for &(target, source, label) in &moves {
            let kind = candidates.get(source).and_then(|candidate| candidate.kind);
            let Some(kind) = kind.filter(|_| !rejected.contains(source)) else {
                continue;
            };
            let candidate = candidates.entry(target).or_default();
            if candidate.assigned.insert(label) {
                grew = true;
            }
            if candidate
                .kind
                .replace(kind)
                .is_some_and(|other| other != kind)
            {
                rejected.insert(target);
            }
        }
    }
    for &(target, source, _) in &moves {
        if !candidates.contains_key(source) || rejected.contains(source) {
            rejected.insert(target);
        }
    }
    candidates.retain(|name, _| !rejected.contains(name));
    if candidates.is_empty() {
        return HashSet::new();
    }

    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::Release { .. } => continue,
                // A move between slots copies the word.
                Instruction::Assign { target, value }
                    if moved_temporary(value).is_some_and(|source| {
                        candidates.contains_key(source) && candidates.contains_key(target.as_str())
                    }) =>
                {
                    continue
                }
                // A discarded value is loaded and dropped.
                Instruction::Eval {
                    value: Operand::Place(place) | Operand::MovePlace(place),
                } if candidates.contains_key(place.as_str()) => continue,
                Instruction::Assign {
                    value:
                        Rvalue::VariantPayload {
                            scrutinee: Operand::Place(place) | Operand::MovePlace(place),
                            variant_name,
                            index,
                        },
                    ..
                } if candidates.contains_key(place.as_str()) => {
                    let candidate = candidates.get_mut(place.as_str()).expect("a candidate");
                    let kind = candidate.kind.expect("an assigned candidate");
                    if *index == 0 && kind.keeps_payload_of(variant_name) {
                        candidate.unpacked_in.push(&block.label);
                    } else {
                        rejected.insert(place);
                    }
                    continue;
                }
                _ => {}
            }
            let mut mentions = Vec::new();
            instruction_place_mentions(instruction, &mut mentions);
            if let Instruction::ReadLoan { target, .. } = instruction {
                mentions.push(target);
            }
            rejected.extend(
                mentions
                    .into_iter()
                    .filter(|place| candidates.contains_key(place)),
            );
        }
        match &block.terminator {
            Terminator::Match {
                scrutinee: Operand::Place(place),
                arms,
                otherwise,
            } if candidates.contains_key(place.as_str()) => {
                let kind = candidates[place.as_str()]
                    .kind
                    .expect("an assigned candidate");
                let tests_slot = arms
                    .iter()
                    .all(|arm| arm.wildcard || kind.tests(arm, otherwise));
                if !tests_slot {
                    rejected.insert(place);
                }
            }
            terminator => rejected.extend(
                terminator_place_mentions(terminator)
                    .into_iter()
                    .filter(|place| candidates.contains_key(place)),
            ),
        }
    }
    candidates.retain(|name, _| !rejected.contains(name));

    let successors = function
        .blocks
        .iter()
        .map(|block| (block.label.as_str(), successors(&block.terminator)))
        .collect::<HashMap<_, _>>();
    let mut slots = candidates
        .into_iter()
        .filter(|(_, candidate)| match candidate.unpacked_in.as_slice() {
            [] => true,
            [block] => !reaches_again_without_assignment(block, &candidate.assigned, &successors),
            // Two copies of one payload would share one box.
            _ => false,
        })
        .map(|(name, _)| name)
        .collect::<HashSet<_>>();
    // Both ends of a move are slots, or neither is.
    let mut shrank = true;
    while shrank {
        shrank = false;
        for (target, source, _) in &moves {
            if slots.contains(source) != slots.contains(target) {
                shrank |= slots.remove(source) | slots.remove(target);
            }
        }
    }
    slots.into_iter().map(str::to_string).collect()
}

#[derive(Default)]
struct Candidate<'a> {
    /// What the temporary would otherwise hold.
    kind: Option<SlotKind>,
    /// The blocks that assign the temporary.
    assigned: HashSet<&'a str>,
    /// The blocks that copy its payload.
    unpacked_in: Vec<&'a str>,
}

fn successors(terminator: &Terminator) -> Vec<&str> {
    match terminator {
        Terminator::Goto(label) => vec![label],
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => vec![then_label, else_label],
        Terminator::ForRange {
            body_label,
            exit_label,
            ..
        } => vec![body_label, exit_label],
        Terminator::Match {
            arms, otherwise, ..
        } => arms
            .iter()
            .map(|arm| arm.label.as_str())
            .chain(std::iter::once(otherwise.as_str()))
            .collect(),
        Terminator::Return(_) | Terminator::AssertFail { .. } | Terminator::Unreachable => {
            Vec::new()
        }
    }
}

/// Whether control can leave `start` and come back to it without passing
/// a block that assigns the temporary. A payload copied twice from one
/// lookup would share one box, so a change to the first copy would show in
/// the second.
fn reaches_again_without_assignment(
    start: &str,
    assigned: &HashSet<&str>,
    successors: &HashMap<&str, Vec<&str>>,
) -> bool {
    let mut seen = HashSet::new();
    let mut pending = successors.get(start).cloned().unwrap_or_default();
    while let Some(block) = pending.pop() {
        if block == start {
            return true;
        }
        if assigned.contains(block) || !seen.insert(block) {
            continue;
        }
        pending.extend(successors.get(block).into_iter().flatten().copied());
    }
    false
}

/// The lookup slots of `function` whose found value native code may lend
/// instead of copying, with the places that hold that value. Lending is a
/// read of the value where it lies in the collection, so each slot here
/// has these properties:
///
/// - Its `get` reads a root `dict` or a lent element of a root `list`.
/// - It and the locals its payload moves along each hold one handle the
///   MIR retains and releases, as `holds_handle` says, and each local is
///   assigned once. Nothing else reads them but
///   a variant test, a scalar payload read into a local `scalar_local`
///   accepts, or the next copy in the chain. The chain's retains and
///   releases stay.
/// - While any of them is still to be read, the function runs only plain
///   steps: a scalar store, the chain's own steps, the end of a loan, and
///   a branch or a match on them. No call, release of another value, store
///   into a collection, or safepoint can change or free the collection.
///
/// The runtime copies the value anyway when the collection may be reached
/// by another thread, so the places hold either form.
pub(super) fn lent_lookup_places(
    function: &MirFunction,
    slots: &HashSet<String>,
    holds_handle: impl Fn(&str) -> bool,
    scalar_local: impl Fn(&str) -> bool,
) -> HashSet<String> {
    let mut assignments = HashMap::<&str, usize>::new();
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        if let Instruction::Assign { target, .. } | Instruction::ReadLoan { target, .. } =
            instruction
        {
            *assignments.entry(target.as_str()).or_default() += 1;
        }
    }
    let params = function
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<HashSet<_>>();
    let assigned_once = |place: &str| {
        assignments.get(place) == Some(&1) && !params.contains(place) && !place.contains('.')
    };
    let mut lent = HashSet::new();
    let mut names = slots.iter().collect::<Vec<_>>();
    names.sort();
    for slot in names {
        if !assigned_once(slot) || !holds_handle(slot) || !reads_root_collection(function, slot) {
            continue;
        }
        // The slot's found value and every local it is copied into.
        let mut chain = vec![slot.as_str()];
        let mut grew = true;
        while grew {
            grew = false;
            for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
                let Instruction::Assign { target, value } = instruction else {
                    continue;
                };
                let source = match value {
                    Rvalue::VariantPayload {
                        scrutinee: Operand::Place(source) | Operand::MovePlace(source),
                        ..
                    } if source == slot => source,
                    Rvalue::Use(Operand::Place(source) | Operand::MovePlace(source))
                        if source != slot =>
                    {
                        source
                    }
                    _ => continue,
                };
                if chain.contains(&source.as_str()) && !chain.contains(&target.as_str()) {
                    chain.push(target);
                    grew = true;
                }
            }
        }
        let held = &chain[1..];
        if held.is_empty()
            || !held
                .iter()
                .all(|place| assigned_once(place) && holds_handle(place))
        {
            continue;
        }
        let members = chain.iter().copied().collect::<HashSet<_>>();
        if only_reads_in_place(function, slot, &members, &scalar_local) {
            lent.extend(chain.iter().map(|place| place.to_string()));
        }
    }
    lent
}

/// Whether the one `get` that assigns `slot` reads a root collection
/// place, either a local `dict` or a loan, so the handle it passes is the
/// one the place holds.
fn reads_root_collection(function: &MirFunction, slot: &str) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| match instruction {
            Instruction::Assign {
                target,
                value:
                    value @ Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                receiver: Some(Operand::Place(receiver)),
                                ..
                            },
                        ..
                    },
            } => target == slot && is_dict_get(value) && !receiver.contains('.'),
            _ => false,
        })
}

/// Whether every mention of `members` is a step the lending allows, and
/// only plain steps run while one of them is still to be read.
fn only_reads_in_place(
    function: &MirFunction,
    slot: &str,
    members: &HashSet<&str>,
    scalar_local: &impl Fn(&str) -> bool,
) -> bool {
    let is_member = |place: &str| members.contains(place.split('.').next().unwrap_or(place));
    // A read of a member: the place it reads, or `None` for a step that
    // reads none.
    let mut reads = HashMap::<(&str, usize), Vec<&str>>::new();
    let mut defines = HashMap::<(&str, usize), &str>::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let point = (block.label.as_str(), index);
            match instruction {
                Instruction::Retain { place } | Instruction::Release { place }
                    if members.contains(place.as_str()) => {}
                Instruction::Assign { target, value } if members.contains(target.as_str()) => {
                    defines.insert(point, target);
                    match value {
                        _ if target == slot => {}
                        Rvalue::VariantPayload {
                            scrutinee: Operand::Place(source) | Operand::MovePlace(source),
                            ..
                        }
                        | Rvalue::Use(Operand::Place(source) | Operand::MovePlace(source))
                            if members.contains(source.as_str()) =>
                        {
                            reads.insert(point, vec![source]);
                        }
                        _ => return false,
                    }
                }
                Instruction::Assign {
                    target,
                    value:
                        Rvalue::VariantPayload {
                            scrutinee: Operand::Place(source),
                            ..
                        },
                } if members.contains(source.as_str()) => {
                    if source == slot || !scalar_local(target) || is_member(target) {
                        return false;
                    }
                    reads.insert(point, vec![source]);
                }
                other => {
                    let mut mentions = Vec::new();
                    instruction_place_mentions(other, &mut mentions);
                    if let Instruction::Assign { target, .. }
                    | Instruction::ReadLoan { target, .. } = other
                    {
                        mentions.push(target);
                    }
                    if mentions.into_iter().any(is_member) {
                        return false;
                    }
                }
            }
        }
        match &block.terminator {
            Terminator::Match {
                scrutinee: Operand::Place(place),
                ..
            } if members.contains(place.as_str()) => {
                reads.insert(
                    (block.label.as_str(), block.instructions.len()),
                    vec![place],
                );
            }
            terminator => {
                if terminator_place_mentions(terminator)
                    .into_iter()
                    .any(is_member)
                {
                    return false;
                }
            }
        }
    }

    // Which members are still to be read before each block, by a backward
    // pass to a fixed point.
    let blocks = function
        .blocks
        .iter()
        .map(|block| (block.label.as_str(), block))
        .collect::<HashMap<_, _>>();
    let mut live_in = HashMap::<&str, HashSet<&str>>::new();
    let mut changed = true;
    while changed {
        changed = false;
        for block in function.blocks.iter().rev() {
            let live_out = live_out_of(block, &live_in);
            let live = live_before(
                blocks[block.label.as_str()],
                live_out,
                &reads,
                &defines,
                None,
            )
            .expect("no visitor refuses");
            let entry = live_in.entry(block.label.as_str()).or_default();
            if !live.is_subset(entry) {
                entry.extend(live);
                changed = true;
            }
        }
    }
    if live_in
        .get(function.entry.as_str())
        .is_some_and(|live| !live.is_empty())
    {
        return false;
    }
    let mut plain = |instruction: &Instruction| plain_step(instruction, members, scalar_local);
    function.blocks.iter().all(|block| {
        let live_out = live_out_of(block, &live_in);
        let terminator_plain = live_out.is_empty()
            || match &block.terminator {
                Terminator::Goto(_) | Terminator::Branch { .. } => true,
                Terminator::Match {
                    scrutinee: Operand::Place(place),
                    ..
                } => members.contains(place.as_str()),
                _ => false,
            };
        terminator_plain
            && live_before(block, live_out, &reads, &defines, Some(&mut plain)).is_some()
    })
}

/// The members still to be read after `block`: those before any of its
/// successors.
fn live_out_of<'f>(
    block: &BasicBlock,
    live_in: &HashMap<&str, HashSet<&'f str>>,
) -> HashSet<&'f str> {
    successors(&block.terminator)
        .into_iter()
        .flat_map(|label| live_in.get(label).cloned().unwrap_or_default())
        .collect()
}

/// The members still to be read before `block`, given those still to be
/// read after it. A `visit` that refuses a step run while one is still to
/// be read gives `None`.
fn live_before<'f>(
    block: &'f BasicBlock,
    live_out: HashSet<&'f str>,
    reads: &HashMap<(&'f str, usize), Vec<&'f str>>,
    defines: &HashMap<(&'f str, usize), &'f str>,
    mut visit: Option<&mut dyn FnMut(&Instruction) -> bool>,
) -> Option<HashSet<&'f str>> {
    let label = block.label.as_str();
    let mut live = live_out;
    if let Some(read) = reads.get(&(label, block.instructions.len())) {
        live.extend(read.iter().copied());
    }
    for (index, instruction) in block.instructions.iter().enumerate().rev() {
        if let Some(visit) = visit.as_mut() {
            if !live.is_empty() && !visit(instruction) {
                return None;
            }
        }
        if let Some(defined) = defines.get(&(label, index)) {
            live.remove(defined);
        }
        if let Some(read) = reads.get(&(label, index)) {
            live.extend(read.iter().copied());
        }
    }
    Some(live)
}

/// Whether `instruction` can run while a lent value is still to be read:
/// it neither calls anything nor changes or frees a collection.
fn plain_step(
    instruction: &Instruction,
    members: &HashSet<&str>,
    scalar_local: &impl Fn(&str) -> bool,
) -> bool {
    let plain_operand = |operand: &Operand| match operand {
        Operand::Place(place) => !members.contains(place.as_str()) && scalar_local(place),
        Operand::MovePlace(_) | Operand::Function { .. } | Operand::Default(_) => false,
        _ => true,
    };
    match instruction {
        Instruction::Retain { place } | Instruction::Release { place } => {
            members.contains(place.as_str())
        }
        Instruction::EndLoan { .. } | Instruction::IndexInRange { .. } => true,
        // The chain's own steps, which the caller checked.
        Instruction::Assign { target, .. } if members.contains(target.as_str()) => true,
        Instruction::Assign { target, value } if scalar_local(target) => match value {
            Rvalue::VariantPayload {
                scrutinee: Operand::Place(source),
                ..
            } => members.contains(source.as_str()),
            Rvalue::Use(operand) | Rvalue::Unary { value: operand, .. } => plain_operand(operand),
            Rvalue::Binary { left, right, .. } => plain_operand(left) && plain_operand(right),
            _ => false,
        },
        _ => false,
    }
}

/// The temporaries of `function` that hold a slice of a list of bytes
/// which only the `str.from_bytes` call right after it reads. Native code
/// makes no slice for them: the call reads the bytes in the list. Each one
/// is assigned once, by the slice of a local list, and is otherwise only
/// released.
pub(super) fn text_byte_slices(function: &MirFunction) -> HashSet<String> {
    let mut slices = HashSet::new();
    for block in &function.blocks {
        for pair in block.instructions.windows(2) {
            let [Instruction::Assign {
                target,
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op: RuntimeOp::List(ListOp::Slice),
                                receiver: Some(Operand::Place(_)),
                                ..
                            },
                        ..
                    },
            }, Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op: RuntimeOp::Host(builtin),
                                receiver: None,
                                ..
                            },
                        args,
                        ..
                    },
                ..
            }] = pair
            else {
                continue;
            };
            let reads_slice = matches!(
                args.as_slice(),
                [MirArg {
                    name: None,
                    value: Operand::Place(place) | Operand::MovePlace(place),
                    ..
                }] if place == target
            );
            if is_temporary(target) && is_str_from_bytes(*builtin) && reads_slice {
                slices.insert(target.as_str());
            }
        }
    }
    if slices.is_empty() {
        return HashSet::new();
    }
    // Each one is assigned once, read once by its call, and released.
    let mut mentions = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Instruction::Release { .. } = instruction {
                continue;
            }
            let mut places = Vec::new();
            instruction_place_mentions(instruction, &mut places);
            if let Instruction::Assign { target, .. } | Instruction::ReadLoan { target, .. } =
                instruction
            {
                places.push(target);
            }
            for place in places {
                if let Some(slice) = slices.get(place.split('.').next().unwrap_or(place)) {
                    *mentions.entry(slice).or_default() += 1;
                }
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            if let Some(slice) = slices.get(place.split('.').next().unwrap_or(place)) {
                *mentions.entry(slice).or_default() += 2;
            }
        }
    }
    let params = function
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<HashSet<_>>();
    slices
        .into_iter()
        .filter(|slice| mentions.get(slice) == Some(&2) && !params.contains(slice))
        .map(str::to_string)
        .collect()
}
