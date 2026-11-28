//! Parts of a split read where the text keeps them.
//!
//! `fields = line.split(" ")` followed by reads of `fields[3]` makes a
//! list of every part, a `str` box each, to read one of them. When the
//! list is only ever read at known positions, in the block that makes it,
//! native code makes no list: each read finds its part in the text and
//! holds it as words (see `str_slots`), the way a loaned element of a
//! `list[str]` is held.
//!
//! The words point into the text that was split, so the text must stay as
//! it is from the split to the end of the last read: the **window**. A
//! callee reaches a local or a parameter only when it is passed in, so
//! only an instruction that names the text can change or free it. Inside
//! the window nothing names it but its release, which is put off to the
//! window's end, and no closure or task of the function captures it. The
//! window holds only assignments that cannot leave the function, releases,
//! lends and loans, so its end is always reached, or the task traps and
//! frees what the function holds as it does anywhere else.
//!
//! A read reports a position the list would not have as the read of the
//! list does, at the same instruction.

use std::collections::{HashMap, HashSet};

use crate::ast::BinaryOp;
use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MirFunction,
    Operand, Rvalue,
};
use crate::runtime_ops::{ListOp, RuntimeOp, StrOp};
use crate::Span;

use super::*;

/// One read of a part: a loan of `list[index]`, or the local an element
/// read of it assigns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PartLoan {
    pub(super) loan: String,
    pub(super) index: i64,
    pub(super) span: Span,
    /// The positions in the block of the read's begin and of its end: the
    /// loan's end, or the release of the local.
    pub(super) begin: usize,
    pub(super) end: usize,
    /// Whether the read is an element read into a local, not a loan.
    pub(super) copied: bool,
}

/// A split whose list is never made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SplitPart {
    /// The block of the split and of every read of its parts.
    pub(super) block: String,
    /// The position of the split in the block.
    pub(super) split: usize,
    /// The local the split assigns and, when that is a temporary moved
    /// into a named local, that local.
    pub(super) lists: Vec<String>,
    /// The position of that move.
    pub(super) alias: Option<usize>,
    /// The `str` that is split.
    pub(super) receiver: String,
    pub(super) separator: String,
    /// For a trim (see [`split_parts`]): which ends lose their whitespace,
    /// as `aura_direct_str_trim_view` counts them.
    pub(super) trim: Option<i64>,
    pub(super) loans: Vec<PartLoan>,
    /// The position of the release of the receiver inside the window.
    pub(super) receiver_release: Option<usize>,
    /// The position of the last loan's end: the end of the window.
    pub(super) window_end: usize,
    /// The releases of the lists, by block and position.
    pub(super) list_releases: Vec<(String, usize)>,
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// Every place `instruction` names: the places it mentions, the target of
/// an assignment, and the loan a loan instruction makes.
fn named_places(instruction: &Instruction) -> Vec<&str> {
    let mut places = Vec::new();
    instruction_place_mentions(instruction, &mut places);
    match instruction {
        Instruction::Assign { target, .. } => places.push(target),
        Instruction::BeginLoan { loan, .. }
        | Instruction::BeginElementLoan { loan, .. }
        | Instruction::BeginReturnedLoan { loan, .. }
        | Instruction::Reborrow { loan, .. }
        | Instruction::ReborrowElement { loan, .. } => places.push(loan),
        _ => {}
    }
    places
}

/// Whether `instruction` may stand inside a window: it cannot leave the
/// function other than by a trap, and it makes nothing that outlives the
/// window with a place of the window in it.
fn stays_in_window(instruction: &Instruction) -> bool {
    match instruction {
        Instruction::Assign { value, .. } => !matches!(
            value,
            Rvalue::Try { .. } | Rvalue::Closure { .. } | Rvalue::StartTask { .. }
        ),
        Instruction::Release { .. }
        | Instruction::BeginElementLoan { .. }
        | Instruction::EndLoan { .. }
        | Instruction::Lend { .. }
        | Instruction::EndLend { .. }
        | Instruction::IndexInRange { .. }
        | Instruction::Safepoint
        | Instruction::SourceLocation { .. }
        | Instruction::SourceName { .. } => true,
        _ => false,
    }
}

/// Whether `instruction` only reads the `str` the loan `loan` selects: a
/// call that takes it as an argument or a receiver, a comparison, or an
/// f-string. Each reads the words, or a box made from them that the loan's
/// end releases.
fn reads_part(instruction: &Instruction, loan: &str) -> bool {
    let Instruction::Assign { target, value } = instruction else {
        return false;
    };
    if root(target) == loan {
        return false;
    }
    let mut mentions = Vec::new();
    instruction_place_mentions(instruction, &mut mentions);
    // The loan itself, never a part of it, and never moved.
    let mut moved = false;
    let mut check = |operand: &Operand| {
        moved |= matches!(operand, Operand::MovePlace(place) if root(place) == loan);
    };
    match value {
        Rvalue::Call { callee, args, .. } => {
            if args.iter().any(|arg| {
                arg.writeback_place.is_some()
                    && matches!(&arg.value, Operand::Place(place) if root(place) == loan)
            }) {
                return false;
            }
            for arg in args {
                check(&arg.value);
            }
            match callee {
                CallTarget::Runtime { receiver, .. } => {
                    if let Some(receiver) = receiver {
                        check(receiver);
                    }
                }
                CallTarget::Name(_) => {}
                // Any other callee names places this does not look into.
                _ => return false,
            }
        }
        Rvalue::Binary {
            op: BinaryOp::Eq | BinaryOp::NotEq,
            left,
            right,
            ..
        } => {
            check(left);
            check(right);
        }
        Rvalue::FormatString { .. } => {}
        _ => return false,
    }
    !moved
        && mentions
            .iter()
            .all(|place| root(place) != loan || *place == loan)
}

/// The splits of `function` whose parts are read where the text keeps
/// them. Each is a split of a `str` local or parameter by a literal whose
/// list is named only by its releases and by reads of one element at a
/// literal position, all in the block of the split: a checked, read-only
/// loan, or an element read into a local that the block then releases.
/// Each read begins once, ends once in that block (the loan's end, or the
/// local's release), and in between its `str` is named only by
/// instructions that read it (see [`reads_part`]). The window from the
/// split to the last read's end holds only instructions that stay in it
/// (see [`stays_in_window`]), none of which names the receiver but its one
/// release, and no closure or task captures the receiver. A named local
/// that several splits fill, as the `fields` of two loops is, is never
/// made only when none of them makes it.
///
/// A trim of such a receiver whose text is read and released that way, in
/// the block of the trim, is here too, with no list and with the local the
/// trim assigns as its one read: the text without its whitespace is a part
/// of the text.
pub(super) fn split_parts(function: &MirFunction) -> Vec<SplitPart> {
    let text = Type::named("str");
    let texts = Type::Named("list".to_string(), vec![text.clone()]);
    let local_is = |name: &str, ty: &Type| {
        function
            .local_types
            .iter()
            .any(|local| local.name == name && local.ty == *ty)
            && !function.params.iter().any(|param| param.name == name)
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
    // The literal a temporary holds at `before` in a block: its one
    // assignment in the function, earlier in that block.
    let literal_of = |temp: &str, block_index: usize, before: usize| {
        let mut assigned = function.blocks.iter().enumerate().flat_map(|(at, block)| {
            block
                .instructions
                .iter()
                .enumerate()
                .filter_map(move |(index, instruction)| match instruction {
                    Instruction::Assign { target, value } if target == temp => {
                        Some((at, index, value))
                    }
                    _ => None,
                })
        });
        match (assigned.next(), assigned.next()) {
            (Some((at, index, Rvalue::Use(Operand::Int(literal)))), None)
                if at == block_index && index < before =>
            {
                Some(*literal)
            }
            _ => None,
        }
    };
    // Where the read of a `str` into the local or loan `loan`, begun at
    // `at` of a block, ends: once, in that block, by the loan's end or the
    // release of the local, with only instructions that read the `str`
    // naming it until then.
    let read_end = |loan: &str, block_index: usize, at: usize, copied: bool| {
        if loan.contains('.') || !local_is(loan, &text) {
            return None;
        }
        let block = &function.blocks[block_index];
        let mut end = None;
        for &(use_block, use_at) in uses_of(loan) {
            if use_block != block_index || use_at < at {
                return None;
            }
            if use_at == at {
                continue;
            }
            match block.instructions.get(use_at) {
                Some(Instruction::EndLoan { loan: ended })
                    if ended == loan && end.is_none() && !copied =>
                {
                    end = Some(use_at);
                }
                Some(Instruction::Release { place })
                    if place == loan && end.is_none() && copied =>
                {
                    end = Some(use_at);
                }
                Some(reader) if end.is_none() && reads_part(reader, loan) => {}
                _ => return None,
            }
        }
        end
    };
    // The read of one element of `list` that the instruction at `at` of a
    // block begins, when it is one.
    let read_of = |list: &str, block_index: usize, at: usize| -> Option<PartLoan> {
        let (loan, index, span, copied) = match function.blocks[block_index].instructions.get(at)? {
            Instruction::BeginElementLoan {
                loan,
                source,
                selector: Operand::Int(index),
                projection,
                mutable: false,
                span,
                checked: true,
            } if source == list && projection.is_empty() => (loan, *index, *span, false),
            Instruction::Assign {
                target,
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op: RuntimeOp::List(ListOp::Index),
                                receiver: Some(Operand::Place(source)),
                                ..
                            },
                        args,
                        ..
                    },
            } if source == list => {
                let [position, MirArg {
                    value: Operand::Int(line),
                    ..
                }, MirArg {
                    value: Operand::Int(column),
                    ..
                }] = args.as_slice()
                else {
                    return None;
                };
                let index = match &position.value {
                    Operand::Int(index) => *index,
                    Operand::Place(temp) => literal_of(temp, block_index, at)?,
                    _ => return None,
                };
                let span = Span {
                    line: usize::try_from(*line).ok()?,
                    column: usize::try_from(*column).ok()?,
                };
                (target, index, span, true)
            }
            _ => return None,
        };
        let index = i64::try_from(index).ok().filter(|index| *index >= 0)?;
        Some(PartLoan {
            loan: loan.clone(),
            index,
            span,
            begin: at,
            end: read_end(loan, block_index, at, copied)?,
            copied,
        })
    };
    // Every split by a literal of a receiver that may be read in place,
    // with the move of its temporary into a named local.
    let mut found = Vec::<(usize, SplitPart)>::new();
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (split, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Assign {
                target,
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op: RuntimeOp::Str(op),
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
            let receiver_is_str = local_is(receiver, &text)
                || function.params.iter().any(|param| {
                    param.name == *receiver
                        && param.ty == text
                        && param.passing == MirReceiverKind::Borrow
                });
            let trim = match op {
                StrOp::Trim => Some(0),
                StrOp::TrimStart => Some(1),
                StrOp::TrimEnd => Some(2),
                _ => None,
            };
            if let (Some(trim), true) = (trim, args.is_empty()) {
                // The text without its whitespace is a part of the text:
                // the local the trim assigns is read as one.
                let read = read_end(target, block_index, split, true).filter(|_| {
                    !receiver.contains('.')
                        && receiver != target
                        && receiver_is_str
                        && !captured.contains(receiver.as_str())
                });
                if let Some(end) = read {
                    found.push((
                        block_index,
                        SplitPart {
                            block: block.label.clone(),
                            split,
                            lists: Vec::new(),
                            alias: None,
                            receiver: receiver.clone(),
                            separator: String::new(),
                            trim: Some(trim),
                            loans: vec![PartLoan {
                                loan: target.clone(),
                                index: 0,
                                span: Span { line: 0, column: 0 },
                                begin: split,
                                end,
                                copied: true,
                            }],
                            receiver_release: None,
                            window_end: end,
                            list_releases: Vec::new(),
                        },
                    ));
                }
                continue;
            }
            let (
                StrOp::Split,
                [MirArg {
                    value: Operand::String(separator),
                    ..
                }],
            ) = (op, args.as_slice())
            else {
                continue;
            };
            // The temporary is assigned here and nowhere else.
            let assigned_once = uses_of(target).iter().all(|&(use_block, use_at)| {
                (use_block, use_at) == (block_index, split)
                    || !matches!(
                        function.blocks[use_block].instructions.get(use_at),
                        Some(Instruction::Assign { target: other, .. }) if root(other) == target
                    )
            });
            if receiver.contains('.')
                || target.contains('.')
                || !receiver_is_str
                || captured.contains(receiver.as_str())
                || !local_is(target, &texts)
                || !assigned_once
            {
                continue;
            }
            let alias = block
                .instructions
                .iter()
                .enumerate()
                .skip(split + 1)
                .find_map(|(index, later)| match later {
                    Instruction::Assign {
                        target: named,
                        value: Rvalue::Use(Operand::MovePlace(moved)),
                    } if moved == target && !named.contains('.') && local_is(named, &texts) => {
                        Some((index, named.clone()))
                    }
                    _ => None,
                });
            let mut lists = vec![target.clone()];
            lists.extend(alias.iter().map(|(_, named)| named.clone()));
            found.push((
                block_index,
                SplitPart {
                    block: block.label.clone(),
                    split,
                    lists,
                    alias: alias.map(|(index, _)| index),
                    receiver: receiver.clone(),
                    separator: separator.clone(),
                    trim: None,
                    loans: Vec::new(),
                    receiver_release: None,
                    window_end: split,
                    list_releases: Vec::new(),
                },
            ));
        }
    }
    // Each use of a list goes to the split that last filled the list in
    // the use's block. A list with a use no split takes is made, by every
    // split that fills it.
    let mut made = HashSet::<String>::new();
    let names = found
        .iter()
        .flat_map(|(_, part)| part.lists.clone())
        .collect::<HashSet<_>>();
    for name in &names {
        for &(use_block, use_at) in uses_of(name) {
            let instruction = function.blocks[use_block].instructions.get(use_at);
            if matches!(instruction, Some(Instruction::Release { place }) if place == name) {
                continue;
            }
            // The split that fills the list here, or the one whose fill
            // this use follows.
            let owner = found
                .iter_mut()
                .filter(|(block, part)| {
                    *block == use_block && part.lists.contains(name) && part.split <= use_at
                })
                .max_by_key(|(_, part)| part.split)
                .map(|(_, part)| part);
            let taken = owner.is_some_and(|part| {
                if use_at == part.split || Some(use_at) == part.alias {
                    return true;
                }
                // A read of the list a later move fills is of that list.
                let filled = part.lists.last() == Some(name)
                    && part.alias.is_none_or(|alias| alias < use_at);
                match read_of(name, use_block, use_at).filter(|_| filled) {
                    Some(read) => {
                        part.loans.push(read);
                        true
                    }
                    None => false,
                }
            });
            if !taken {
                made.insert(name.clone());
            }
        }
    }
    // The window of each split.
    for (block_index, part) in &mut found {
        let block = &function.blocks[*block_index];
        part.loans.sort_by_key(|loan| loan.begin);
        part.window_end = part
            .loans
            .iter()
            .map(|loan| loan.end)
            .max()
            .unwrap_or(part.split);
        let mut whole = !part.loans.is_empty();
        for (index, inside) in block
            .instructions
            .iter()
            .enumerate()
            .take(part.window_end + 1)
            .skip(part.split + 1)
        {
            whole &= stays_in_window(inside);
            if !named_places(inside)
                .iter()
                .any(|place| root(place) == part.receiver)
            {
                continue;
            }
            match inside {
                Instruction::Release { place } if *place == part.receiver => {
                    whole &= part.receiver_release.replace(index).is_none();
                }
                _ => whole = false,
            }
        }
        if !whole {
            made.extend(part.lists.iter().cloned());
            // A trim fills no list: it alone makes its text.
            part.loans.clear();
        }
    }
    // A split that makes one of its lists makes them all, and so does
    // every split that fills one of those.
    loop {
        let spread = found
            .iter()
            .filter(|(_, part)| part.lists.iter().any(|list| made.contains(list)))
            .flat_map(|(_, part)| part.lists.clone())
            .filter(|list| !made.contains(list))
            .collect::<Vec<_>>();
        if spread.is_empty() {
            break;
        }
        made.extend(spread);
    }
    found.retain(|(_, part)| {
        !part.loans.is_empty() && !part.lists.iter().any(|list| made.contains(list))
    });
    for (_, part) in &mut found {
        for list in &part.lists {
            for &(use_block, use_at) in uses_of(list) {
                let block = &function.blocks[use_block];
                if matches!(block.instructions.get(use_at), Some(Instruction::Release { place }) if place == list)
                {
                    part.list_releases.push((block.label.clone(), use_at));
                }
            }
        }
    }
    found.into_iter().map(|(_, part)| part).collect()
}

/// What native code does at one instruction of a split whose list is never
/// made.
#[derive(Clone, Debug)]
pub(super) enum Step {
    /// The split: keep the words of the receiver.
    Split {
        receiver: String,
        ptr: Var,
        len: Var,
    },
    /// A trim: find the text without its whitespace in the receiver.
    Trim {
        receiver: String,
        local: String,
        kind: i64,
    },
    /// The move of the list, a release of it, or the receiver's release
    /// inside the window: nothing.
    Nothing,
    /// A loan's begin: find the part.
    Begin {
        loan: String,
        separator: String,
        index: i64,
        span: Span,
        ptr: Var,
        len: Var,
    },
    /// A loan's end: release the box a read made of the part, and at the
    /// window's end the receiver, when its release was put off.
    End {
        loan: String,
        release: Option<(String, usize)>,
    },
}

impl FunctionCompiler<'_> {
    /// Plans the splits of `function` whose parts are read where the text
    /// keeps them (see [`split_parts`]), leaving out each whose receiver
    /// this body cannot hold still for the window: it must be an element
    /// slot of a list the window does not name and no `Lend` fills, a
    /// borrowed parameter, or a local that owns its box. A list or a loan
    /// that another way of compiling the function claims keeps its list
    /// too.
    pub(super) fn plan_split_parts(&mut self, function: &MirFunction) {
        let mut planned = Vec::new();
        for part in split_parts(function) {
            let Some(block) = function
                .blocks
                .iter()
                .find(|block| block.label == part.block)
            else {
                continue;
            };
            let receiver = part.receiver.as_str();
            let held = if let Some(list) = self.str_element_lists.get(receiver) {
                let lent = function
                    .blocks
                    .iter()
                    .flat_map(|block| &block.instructions)
                    .any(|instruction| {
                        matches!(instruction, Instruction::Lend { temp, .. } if temp == list)
                    });
                !lent
                    && !block.instructions[part.split..=part.window_end]
                        .iter()
                        .any(|instruction| {
                            named_places(instruction)
                                .iter()
                                .any(|place| root(place) == list)
                        })
            } else if function.params.iter().any(|param| param.name == receiver) {
                true
            } else {
                !self.str_slots.contains_key(receiver) && self.owns_counted_str(receiver)
            };
            // A local an element read of the list assigns may be an element
            // slot of the list; it reads the text instead.
            let claimed = |name: &str| {
                (self.str_slots.contains_key(name)
                    && !self
                        .str_element_lists
                        .get(name)
                        .is_some_and(|list| part.lists.contains(list)))
                    || self.block_loans.contains_key(name)
                    || self.fused_entry_copies.contains_key(name)
                    || self.fused_entry_slots.contains_key(name)
                    || self.lent_entry_copies.contains(name)
                    || self.shadow_read_targets.contains(name)
                    || self.node_read_loans.contains(name)
                    || self.node_read_targets.contains(name)
                    || self.push_entry_loans.contains(name)
                    || self.called_loan_reads.values().any(|loan| loan == name)
                    || self.element_calls.contains_key(name)
                    || self.record_lends.contains_key(name)
                    || self.lent_elements.contains_key(name)
                    || self.lent_places.contains(name)
                    || self.str_literal_lists.contains(name)
                    || self.key_loops.contains_key(name)
                    || self.entry_loops.contains_key(name)
                    || self.result_slots.contains_key(name)
                    || self.lookup_slots.contains(name)
                    || self.json_views.contains(name)
                    || self.view_selector_vars.contains_key(name)
                    || self
                        .list_snapshots
                        .iter()
                        .any(|snapshot| snapshot.region.list == name)
            };
            let reads = held
                && !part.lists.iter().any(|list| claimed(list))
                && !part.loans.iter().any(|loan| claimed(&loan.loan));
            planned.push((reads, part));
        }
        // A part of a text that is itself a part would have to outlive the
        // read it is split from, so such a split makes its list.
        let parts = planned
            .iter()
            .filter(|(reads, _)| *reads)
            .flat_map(|(_, part)| part.loans.iter().map(|loan| loan.loan.clone()))
            .collect::<HashSet<_>>();
        for (reads, part) in &mut planned {
            *reads &= !parts.contains(&part.receiver);
        }
        // A list that one split makes is made by every split that fills
        // it, since its releases must then free it.
        let mut made = HashSet::new();
        loop {
            let before = made.len();
            for (reads, part) in &mut planned {
                *reads &= !part.lists.iter().any(|list| made.contains(list));
                if !*reads {
                    made.extend(part.lists.iter().cloned());
                }
            }
            if made.len() == before {
                break;
            }
        }
        let planned = planned
            .into_iter()
            .filter_map(|(reads, part)| reads.then_some(part));
        for part in planned {
            let declare = |this: &mut Self| {
                let variable = Var(this.next_variable_index as u32);
                this.next_variable_index += 1;
                this.builder.declare_var(variable, types::I64);
                let zero = this.builder.iconst(types::I64, 0);
                this.builder.def_var(variable, zero);
                variable
            };
            let (ptr, len) = (declare(self), declare(self));
            let at = |index: usize| (part.block.clone(), index);
            if part.trim.is_none() {
                self.split_part_steps.insert(
                    at(part.split),
                    Step::Split {
                        receiver: part.receiver.clone(),
                        ptr,
                        len,
                    },
                );
            }
            for index in part.alias.into_iter().chain(part.receiver_release) {
                self.split_part_steps.insert(at(index), Step::Nothing);
            }
            for release in &part.list_releases {
                self.split_part_steps.insert(release.clone(), Step::Nothing);
            }
            for loan in &part.loans {
                let begin = match part.trim {
                    Some(kind) => Step::Trim {
                        receiver: part.receiver.clone(),
                        local: loan.loan.clone(),
                        kind,
                    },
                    None => Step::Begin {
                        loan: loan.loan.clone(),
                        separator: part.separator.clone(),
                        index: loan.index,
                        span: loan.span,
                        ptr,
                        len,
                    },
                };
                self.split_part_steps.insert(at(loan.begin), begin);
                let release = part
                    .receiver_release
                    .filter(|_| loan.end == part.window_end)
                    .map(|index| (part.receiver.clone(), index));
                self.split_part_steps.insert(
                    at(loan.end),
                    Step::End {
                        loan: loan.loan.clone(),
                        release,
                    },
                );
                // An element read into a local may already be an element
                // slot of the list that is not made; it reads the text now.
                self.str_element_lists.remove(&loan.loan);
                self.split_part_locals.insert(loan.loan.clone());
                let slot = str_slots::StrSlot::local(declare(self), declare(self));
                self.str_slots.insert(loan.loan.clone(), slot);
            }
        }
    }

    /// Refuses to leave out the instruction at `index` of the block `label`
    /// when it is a step of a split whose list is never made: a fused run
    /// of instructions that starts before it does not know the list is
    /// missing.
    pub(super) fn refuse_skipped_split_part_step(
        &self,
        label: &str,
        index: usize,
    ) -> std::result::Result<(), String> {
        let at = (label.to_string(), index);
        if self.split_part_steps.is_empty()
            || !self.split_part_steps.contains_key(&at)
            || self.split_part_done.contains(&at)
        {
            return Ok(());
        }
        Err(format!(
            "direct backend skipped a read of a split part in `{label}`"
        ))
    }

    /// The bytes of the receiver of a split or a trim: the words this body
    /// holds it as, or those of its box.
    fn split_receiver_words(&mut self, receiver: &str) -> std::result::Result<(Val, Val), String> {
        let operand = Operand::Place(receiver.to_string());
        match self.str_slot_words(&operand) {
            Some(words) => Ok(words),
            None => {
                let held = self.load_place(receiver)?.values[0];
                Ok(self.str_box_words(held))
            }
        }
    }

    /// Compiles the instruction at `index` of the block `label` when it is
    /// a step of a split whose list is never made.
    pub(super) fn compile_split_part_step(
        &mut self,
        label: &str,
        index: usize,
    ) -> std::result::Result<bool, String> {
        if self.split_part_steps.is_empty() {
            return Ok(false);
        }
        let Some(step) = self
            .split_part_steps
            .get(&(label.to_string(), index))
            .cloned()
        else {
            return Ok(false);
        };
        self.split_part_done.insert((label.to_string(), index));
        match step {
            Step::Nothing => {}
            Step::Split { receiver, ptr, len } => {
                let words = self.split_receiver_words(&receiver)?;
                self.builder.def_var(ptr, words.0);
                self.builder.def_var(len, words.1);
            }
            Step::Trim {
                receiver,
                local,
                kind,
            } => {
                let (text, text_len) = self.split_receiver_words(&receiver)?;
                let kind = self.builder.iconst(types::I64, kind);
                let out = self.builder.create_sized_stack_slot(8, 3);
                let out = self.builder.stack_addr(types::I64, out, 0);
                let part = self
                    .builder
                    .call(
                        self.runtime[RuntimeFn::StrTrimView],
                        &[text, text_len, kind, out],
                    )
                    .results[0];
                let part_len = self.builder.load(types::I64, MemFlags::trusted(), out, 0);
                self.define_str_slot(&local, part, part_len)?;
            }
            Step::Begin {
                loan,
                separator,
                index,
                span,
                ptr,
                len,
            } => {
                let (separator_ptr, separator_len) = self.string_constant(separator.as_bytes())?;
                let (line, column) = self.span_values(Some(span));
                let index = self.builder.iconst(types::I64, index);
                let out = self.builder.create_sized_stack_slot(8, 3);
                let out = self.builder.stack_addr(types::I64, out, 0);
                let text = self.builder.use_var(ptr);
                let text_len = self.builder.use_var(len);
                let part = self
                    .builder
                    .call(
                        self.runtime[RuntimeFn::StringSplitPartWords],
                        &[
                            text,
                            text_len,
                            separator_ptr,
                            separator_len,
                            index,
                            line,
                            column,
                            out,
                        ],
                    )
                    .results[0];
                let part_len = self.builder.load(types::I64, MemFlags::trusted(), out, 0);
                self.define_str_slot(&loan, part, part_len)?;
            }
            Step::End { loan, release } => {
                // A read that took the part as a box made one.
                self.release_root(&loan)?;
                if let Some((receiver, at)) = release {
                    self.split_part_steps.remove(&(label.to_string(), at));
                    let release = Instruction::Release { place: receiver };
                    let compiled = self.compile_instruction(label, at, &release);
                    self.split_part_steps
                        .insert((label.to_string(), at), Step::Nothing);
                    compiled?;
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "native_codegen_split_parts_tests.rs"]
mod tests;
