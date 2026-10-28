//! Element loans generated code follows to their place (typed-layout step
//! 2e, part 2).
//!
//! `counters[i].count += 1` begins a loan of `counters[i].count`, reads the
//! count through it, writes the sum through it, and ends it. Each read and
//! write used to walk the element path in the runtime. A loan that selects
//! one element of a local list, or of a list in a field box of a class kept
//! in place, is followed in generated code once, when it begins:
//!
//! - an element of a list that keeps its elements in boxes of its own (the
//!   `Boxes` form) is its box, and a scalar or plain-record place inside a
//!   class element kept in place lies at an offset in the element's block,
//!   after one access-code check each;
//! - an element of a list of scalars lies in the list's storage;
//! - a box slot, an element's or a field box's, is lent to one runtime
//!   operation as it is, as a field lend lends a field's box;
//! - the box in such a slot is passed as it is to a function that borrows
//!   the parameter, as a local list's lent element is (see
//!   `native_codegen_elements.rs`), when the loan only reads its place.
//!
//! A read-only loan that selects inside another one, such as `row[i]` for
//! `view row = rows[slot]`, selects from the list in its parent's slot.
//!
//! A loan that may change its place also checks the position against the
//! list's room, which only storage no other value shares has: after
//! `copy()`, the two lists share their storage, and their boxes, until
//! either changes, so a change in place would reach both.
//!
//! The loan keeps the place's address in a variable, 0 when a check
//! failed. Each read, write, and lend checks it, and a loan that missed
//! takes the runtime walk as before. Its probe counts the miss, so a list
//! that loans miss often enough moves its elements into boxes of its own
//! (see `aura_direct_element_path_promote`). Only a loan whose every use is
//! one of these is followed, and while it lives nothing else names its list
//! (the MIR's loan rules), so the address stays valid until it ends.

use std::collections::{HashMap, HashSet};

use super::classes::{receives_lent_field, ClassSlot, FieldLend, SlotKind};
use super::plain::PlainLeaf;
use super::*;
use crate::mir::{instruction_place_mentions, terminator_place_mentions};

/// How a function uses one element loan generated code may follow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoanUse {
    /// Whether the loan only lends its place, or passes it to borrowing
    /// parameters, rather than reading and writing it.
    lends: bool,
    mutable: bool,
    /// The loan a read-only nested selection begins on.
    parent: Option<String>,
}

/// The element loans of `function` generated code may follow, and the
/// temporaries their lends lend to one runtime operation.
pub(super) fn native_element_loans<'f>(
    function: &'f MirFunction,
    borrowed: impl Fn(&'f Instruction) -> Vec<&'f Operand>,
) -> (HashMap<String, LoanUse>, HashSet<String>) {
    let mut sources = HashMap::new();
    let mut rejected = HashSet::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Instruction::BeginElementLoan {
                loan,
                source,
                mutable,
                ..
            } = instruction
            {
                let begun = (root(source).to_string(), *mutable);
                if sources.insert(loan.clone(), begun).is_some() {
                    rejected.insert(loan.clone());
                }
            }
        }
    }
    // A loan that begins on another loan is a reborrow in all but name.
    rejected.extend(
        sources
            .iter()
            .filter(|(_, (source, _))| sources.contains_key(source))
            .map(|(loan, _)| loan.clone()),
    );
    // A read-only nested selection names the list its parent selects from.
    let mut parents = HashMap::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Instruction::ReborrowElement {
                loan,
                parent,
                mutable: false,
                ..
            } = instruction
            {
                let Some((source, _)) = sources.get(parent).cloned() else {
                    continue;
                };
                if sources.insert(loan.clone(), (source, false)).is_some() {
                    rejected.insert(loan.clone());
                }
                parents.insert(loan.clone(), parent.clone());
            }
        }
    }
    let mut accesses = HashSet::new();
    let mut lends = HashSet::new();
    // The loans passed to borrowing parameters or reborrowed, which must
    // not change their place.
    let mut reads = HashSet::new();
    let mut lend_temps = HashSet::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let mut mentions = Vec::new();
            instruction_place_mentions(instruction, &mut mentions);
            if let Instruction::Assign { target, .. } = instruction {
                mentions.push(target);
            }
            // The one mention of a loan the instruction may make.
            let allowed = match instruction {
                Instruction::ReadLoan { loan, .. }
                | Instruction::WriteLoan { loan, .. }
                | Instruction::EndLoan { loan } => Some(loan.as_str()),
                Instruction::Lend { place, .. } | Instruction::EndLend { place, .. } => {
                    Some(place.as_str())
                }
                _ => None,
            };
            let passed = passed_loan(instruction, &borrowed, &sources);
            let reborrowed = match instruction {
                Instruction::ReborrowElement { parent, .. } => Some(parent.as_str()),
                _ => None,
            };
            let allowed = allowed.or(passed).or(reborrowed);
            let mut allowed_seen = false;
            for mention in mentions {
                let loan = root(mention);
                let Some((source, _)) = sources.get(loan) else {
                    continue;
                };
                if allowed_seen || allowed != Some(mention) || mention != loan {
                    rejected.insert(loan.to_string());
                    continue;
                }
                allowed_seen = true;
                match instruction {
                    Instruction::ReadLoan { .. } | Instruction::WriteLoan { .. } => {
                        accesses.insert(loan.to_string());
                    }
                    Instruction::Lend { temp, .. } => {
                        if lends_to_one_operation(&block.instructions, index, temp, loan, source) {
                            lends.insert(loan.to_string());
                            lend_temps.insert(temp.clone());
                        } else {
                            rejected.insert(loan.to_string());
                        }
                    }
                    // A nested selection takes the runtime walk from the
                    // list's root, which an immutable loan leaves as it is.
                    Instruction::ReborrowElement { .. } => {
                        reads.insert(loan.to_string());
                    }
                    Instruction::Assign { .. } => {
                        lends.insert(loan.to_string());
                        reads.insert(loan.to_string());
                    }
                    _ => {}
                }
            }
        }
        for mention in terminator_place_mentions(&block.terminator) {
            if sources.contains_key(root(mention)) {
                rejected.insert(root(mention).to_string());
            }
        }
    }
    let mut uses = sources
        .into_iter()
        .filter(|(loan, _)| !rejected.contains(loan))
        .filter(|(loan, (_, mutable))| !(*mutable && reads.contains(loan)))
        .filter(|(loan, _)| accesses.contains(loan) != lends.contains(loan))
        .map(|(loan, (_, mutable))| {
            let lends = lends.contains(&loan);
            let parent = parents.get(&loan).cloned();
            (
                loan,
                LoanUse {
                    lends,
                    mutable,
                    parent,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    // A nested selection is followed only from a parent that lends its box.
    loop {
        let orphans = uses
            .iter()
            .filter(|(_, usage)| {
                usage.parent.as_ref().is_some_and(|parent| {
                    !uses
                        .get(parent)
                        .is_some_and(|parent| parent.lends && !parent.mutable)
                })
            })
            .map(|(loan, _)| loan.clone())
            .collect::<Vec<_>>();
        if orphans.is_empty() {
            break;
        }
        for orphan in orphans {
            uses.remove(&orphan);
        }
    }
    (uses, lend_temps)
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// The loan `instruction` passes whole to a parameter a function borrows,
/// when the call names nothing else the loan's list lies in: the callee
/// reaches the list only through what it is passed, so it cannot change
/// the list while it holds the element's box.
fn passed_loan<'f>(
    instruction: &'f Instruction,
    borrowed: &impl Fn(&'f Instruction) -> Vec<&'f Operand>,
    sources: &HashMap<String, (String, bool)>,
) -> Option<&'f str> {
    let Instruction::Assign {
        target,
        value: Rvalue::Call {
            callee: CallTarget::Name(_),
            ..
        },
    } = instruction
    else {
        return None;
    };
    let mut passed = borrowed(instruction)
        .into_iter()
        .filter_map(|operand| match operand {
            Operand::Place(place) => sources
                .get(place.as_str())
                .map(|(source, _)| (place, source)),
            _ => None,
        });
    let (loan, source) = passed.next()?;
    if passed.next().is_some() || root(target) == source {
        return None;
    }
    let mut mentions = Vec::new();
    instruction_place_mentions(instruction, &mut mentions);
    mentions
        .iter()
        .all(|mention| root(mention) != source)
        .then_some(loan.as_str())
}

/// Whether the `Lend` at `index` lends `loan`'s place to one runtime
/// operation, as a field lend does (see `classes::field_lend_temps`):
/// before its `EndLend`, only that operation, with `temp` as its receiver,
/// and facts about `temp` name `temp`, the loan, or its list's root.
fn lends_to_one_operation(
    instructions: &[Instruction],
    index: usize,
    temp: &str,
    loan: &str,
    source: &str,
) -> bool {
    let end = instructions[index + 1..].iter().position(
        |later| matches!(later, Instruction::EndLend { temp: ended, .. } if ended == temp),
    );
    let Some(end) = end else {
        return false;
    };
    let body = &instructions[index + 1..index + 1 + end];
    let receives = |instruction: &Instruction| {
        receives_lent_field(instruction, temp, loan)
            && receives_lent_field(instruction, temp, source)
    };
    let calls = body
        .iter()
        .filter(|instruction| receives(instruction))
        .count();
    let rest_stays_away = body.iter().all(|instruction| {
        receives(instruction)
            || matches!(
                instruction,
                Instruction::IndexInRange { list, index }
                    if list == temp && ![temp, loan, source].contains(&root(index))
            )
    });
    calls == 1 && rest_stays_away
}

/// An element loan generated code followed when it began.
#[derive(Clone, Debug)]
pub(super) struct NativeLoan {
    /// The address of the loan's place, or 0 when generated code could not
    /// follow the loan.
    address: Var,
    target: LoanTarget,
}

#[derive(Clone, Debug)]
enum LoanTarget {
    /// A place the loan reads and writes.
    Place(LoanPlace),
    /// A box slot at the address, which the loan lends: an element of a
    /// list of boxes, or a field box of a class element.
    Slot,
}

#[derive(Clone, Debug)]
enum LoanPlace {
    /// A scalar element of a list that keeps its elements unboxed, at the
    /// address.
    Scalar(ScalarElement),
    /// A scalar or plain-record place inside a class element kept in place:
    /// the leaves of the place, in the block at the address.
    Leaves(Vec<PlainLeaf>),
    /// The `int64` value of a dict entry, in the runtime value at the
    /// address (see `aura_direct_map_value_slot`).
    MapValue,
}

/// Where the list a loan selects from lies.
enum LoanList {
    Local(String),
    /// In the box slot at the address a followed parent loan keeps, 0 when
    /// the parent missed.
    Loan(Var),
    /// In the field box at `slot` of the class instance in `root`.
    Field {
        root: String,
        slot: ClassSlot,
    },
}

impl FunctionCompiler<'_> {
    /// Follows the element loan `loan`, whose view was just recorded, to its
    /// place, when the function uses the loan only as the module docs say.
    /// Returns whether it did; the probe a missed loan makes checks the
    /// selection as the loan's beginning always did.
    pub(super) fn begin_native_element_loan(
        &mut self,
        loan: &str,
    ) -> std::result::Result<bool, String> {
        let Some(usage) = self.native_loan_uses.get(loan).cloned() else {
            return Ok(false);
        };
        let parent = match &usage.parent {
            Some(parent) => match self.native_loans.get(parent) {
                Some(NativeLoan {
                    address,
                    target: LoanTarget::Slot,
                }) => Some((parent.clone(), *address)),
                _ => return Ok(false),
            },
            None => None,
        };
        let Some(view) = self.view_places.get(loan) else {
            return Ok(false);
        };
        let [alternative] = view.alternatives.as_slice() else {
            return Ok(false);
        };
        let Some(selector) = alternative.elements.last() else {
            return Ok(false);
        };
        if (parent.is_none() && alternative.elements.len() != 1)
            || !alternative.conditions.is_empty()
            || !alternative.union_payloads.is_empty()
        {
            return Ok(false);
        }
        let place = alternative.place.clone();
        let selector = selector.clone();
        if selector.kind == DirectElementKind::Dict {
            return match parent {
                None => self.begin_native_entry_loan(loan, &usage, &place, &selector),
                Some(_) => Ok(false),
            };
        }
        let (list, list_ty) = match parent {
            Some((parent, address)) => (LoanList::Loan(address), self.local_type(&parent)?),
            None => {
                let Some(list) = self.loan_list(&place, "list")? else {
                    return Ok(false);
                };
                (list, self.type_of_place(&place)?)
            }
        };
        let Type::Named(name, args) = direct_type_to_type(&list_ty) else {
            return Ok(false);
        };
        let [element] = args.as_slice() else {
            return Ok(false);
        };
        if name != "list" {
            return Ok(false);
        }
        let loan_ty = self.local_type(loan)?;
        let Some((target, class)) =
            self.loan_target(element, &selector.projection, &loan_ty, usage.lends)?
        else {
            return Ok(false);
        };

        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let address = self.declare_word_var();
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        let list_box = self.load_loan_list(&list, slow_block)?;
        let index = self.builder.use_var(selector.variable);
        let place_address = match &target {
            LoanTarget::Place(LoanPlace::MapValue) => {
                return Err(format!(
                    "direct backend followed list loan `{loan}` to a dict entry"
                ));
            }
            LoanTarget::Place(LoanPlace::Scalar(kind)) => {
                let (access, stride) = scalar_list_slot(*kind);
                let fast_block = self.builder.create_block();
                self.emit_list_access_check(list_box, access, &layout, slow_block, fast_block);
                self.builder.switch_to_block(fast_block);
                self.builder.seal_block(fast_block);
                let position =
                    self.emit_list_view_bounds_check(list_box, index, &layout, slow_block);
                if usage.mutable {
                    self.emit_unshared_storage_check(list_box, position, slow_block);
                }
                let elements =
                    self.builder
                        .load(types::I64, flags, list_box, layout.elements_offset);
                self.scalar_list_element(elements, position, stride)
            }
            LoanTarget::Place(LoanPlace::Leaves(_)) | LoanTarget::Slot => {
                use aura_runtime::native_runtime::DirectListAccess;
                let fast_block = self.builder.create_block();
                self.emit_list_access_check(
                    list_box,
                    DirectListAccess::Boxes,
                    &layout,
                    slow_block,
                    fast_block,
                );
                self.builder.switch_to_block(fast_block);
                self.builder.seal_block(fast_block);
                let position =
                    self.emit_list_view_bounds_check(list_box, index, &layout, slow_block);
                // Another list may share the boxes since the view was last
                // refreshed, and a change in place would reach it too.
                if usage.mutable {
                    self.emit_unshared_storage_check(list_box, position, slow_block);
                }
                let elements =
                    self.builder
                        .load(types::I64, flags, list_box, layout.elements_offset);
                let element_slot = self.scalar_list_element(elements, position, 8);
                match &class {
                    None => element_slot,
                    Some(slot) => {
                        let element = self.builder.load(types::I64, flags, element_slot, 0);
                        let class_block = self.builder.create_block();
                        self.emit_class_check(element, slot.access, slow_block, class_block);
                        self.builder.switch_to_block(class_block);
                        self.builder.seal_block(class_block);
                        let block = self.class_block(element);
                        match target {
                            LoanTarget::Slot => {
                                self.builder.iadd_imm(block, i64::from(slot.offset))
                            }
                            _ => block,
                        }
                    }
                }
            }
        };
        self.builder.def_var(address, place_address);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        // Only a list of boxes, or a list of instances or lists that may
        // become one, gains from counting the miss.
        let promotes = !matches!(target, LoanTarget::Place(LoanPlace::Scalar(_)));
        self.emit_element_path_probe(loan, promotes)?;
        let missed = self.builder.iconst(types::I64, 0);
        self.builder.def_var(address, missed);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.native_loans
            .insert(loan.to_string(), NativeLoan { address, target });
        Ok(true)
    }

    /// Follows `loan`, which reads and writes the `int64` value of a dict
    /// entry, to the runtime value the dict keeps it in: the runtime finds
    /// the entry once, when the loan begins, and each read and write goes
    /// to it directly instead of walking the path again. A dict shared with
    /// another thread, or a missing key, takes the runtime walk as before,
    /// which reports the key.
    fn begin_native_entry_loan(
        &mut self,
        loan: &str,
        usage: &LoanUse,
        place: &str,
        selector: &DirectElementSelector,
    ) -> std::result::Result<bool, String> {
        if usage.lends
            || !selector.projection.is_empty()
            || self.local_type(loan)? != DirectType::Scalar(ScalarKind::Int64)
        {
            return Ok(false);
        }
        let find = match &selector.selector_ty {
            DirectType::Opaque(_) => RuntimeFn::MapValueSlot,
            DirectType::Scalar(ScalarKind::Int64) => RuntimeFn::MapValueSlotInt64,
            _ => return Ok(false),
        };
        let Some(map) = self.loan_list(place, "dict")? else {
            return Ok(false);
        };
        let address = self.declare_word_var();
        let slow_block = self.builder.create_block();
        let found_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        let map_box = self.load_loan_list(&map, slow_block)?;
        let key = self.builder.use_var(selector.variable);
        let mutable = self.builder.iconst(types::I64, i64::from(usage.mutable));
        let slot = self
            .builder
            .call(self.runtime[find], &[map_box, key, mutable])
            .results[0];
        self.builder.brif(slot, found_block, &[], slow_block, &[]);

        self.builder.switch_to_block(found_block);
        self.builder.seal_block(found_block);
        self.builder.def_var(address, slot);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        // The probe boxes a scalar key for the runtime walk, so its box is
        // released here, in the block that made it.
        let baseline = self.layout_scratch.clone();
        self.emit_element_path_probe(loan, false)?;
        self.release_temporary_owned_since(&baseline);
        let missed = self.builder.iconst(types::I64, 0);
        self.builder.def_var(address, missed);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.native_loans.insert(
            loan.to_string(),
            NativeLoan {
                address,
                target: LoanTarget::Place(LoanPlace::MapValue),
            },
        );
        Ok(true)
    }

    /// The box of the list or dict a followed loan selects from, after the
    /// checks that branch to `slow_block` when generated code cannot reach
    /// it.
    fn load_loan_list(
        &mut self,
        list: &LoanList,
        slow_block: Blk,
    ) -> std::result::Result<Val, String> {
        let flags = MemFlags::trusted();
        let list_box = match list {
            LoanList::Local(name) => self.load_root(name)?.values[0],
            LoanList::Loan(address) => {
                let address = self.builder.use_var(*address);
                let held_block = self.builder.create_block();
                self.builder.brif(address, held_block, &[], slow_block, &[]);
                self.builder.switch_to_block(held_block);
                self.builder.seal_block(held_block);
                let list = self.builder.load(types::I64, flags, address, 0);
                // A moved-out field takes the runtime walk, which reports it.
                let present_block = self.builder.create_block();
                self.builder.brif(list, present_block, &[], slow_block, &[]);
                self.builder.switch_to_block(present_block);
                self.builder.seal_block(present_block);
                list
            }
            LoanList::Field { root, slot } => {
                let object = self.load_root(root)?.values[0];
                let fast_block = self.builder.create_block();
                self.emit_class_check(object, slot.access, slow_block, fast_block);
                self.builder.switch_to_block(fast_block);
                self.builder.seal_block(fast_block);
                let block = self.class_block(object);
                let field = self.builder.load(types::I64, flags, block, slot.offset);
                // A moved-out field takes the runtime walk, which reports it.
                let present_block = self.builder.create_block();
                self.builder
                    .brif(field, present_block, &[], slow_block, &[]);
                self.builder.switch_to_block(present_block);
                self.builder.seal_block(present_block);
                field
            }
        };
        Ok(list_box)
    }

    /// Branches to `slow_block` unless the storage of `list_box` is unshared,
    /// so an element at `position` may change in place. Room covers every
    /// element of unshared storage, and a list whose storage another value
    /// shares has none: any access, or a read of a held box, takes it back
    /// until a write finds the storage unshared again.
    fn emit_unshared_storage_check(&mut self, list_box: Val, position: Val, slow_block: Blk) {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let capacity = self
            .builder
            .uload32(MemFlags::trusted(), list_box, layout.capacity_offset);
        let unshared = self
            .builder
            .icmp(IntCC::UnsignedLessThan, position, capacity);
        let own_block = self.builder.create_block();
        self.builder.brif(unshared, own_block, &[], slow_block, &[]);
        self.builder.switch_to_block(own_block);
        self.builder.seal_block(own_block);
    }

    /// The list a followed loan selects from, at `place`: a local list, or a
    /// field box of a local class instance kept in place.
    fn loan_list(
        &mut self,
        place: &str,
        collection: &str,
    ) -> std::result::Result<Option<LoanList>, String> {
        let root = root(place);
        // A local that may hold a box it does not own is never followed.
        if self.lent_elements.contains_key(root)
            || self.lent_temps.contains_key(root)
            || self.lookup_slots.contains(root)
            || self.lent_places.contains(root)
        {
            return Ok(None);
        }
        if root == place {
            let is_list = matches!(
                self.variable_types.get(place),
                Some(DirectType::Opaque(Type::Named(name, _))) if name == collection
            );
            return Ok(is_list.then(|| LoanList::Local(place.to_string())));
        }
        let Some((root, slot, _)) = self.class_place(place)? else {
            return Ok(None);
        };
        Ok(matches!(slot.kind, SlotKind::Handle).then_some(LoanList::Field { root, slot }))
    }

    /// What a followed loan of `projection` inside a list `element` reaches,
    /// with the slot of the element's class when the projection names a
    /// place in it. A loan that only reads and writes reaches a scalar or
    /// plain-record place, and one that only lends reaches a box.
    fn loan_target(
        &mut self,
        element: &Type,
        projection: &str,
        loan_ty: &DirectType,
        lends: bool,
    ) -> std::result::Result<Option<(LoanTarget, Option<ClassSlot>)>, String> {
        if projection.is_empty() {
            if !lends {
                return Ok(ScalarElement::of(element)
                    .filter(|_| matches!(loan_ty, DirectType::Scalar(_)))
                    .map(|kind| (LoanTarget::Place(LoanPlace::Scalar(kind)), None)));
            }
            // A list of strings keeps its copying reads (see
            // `elements::lent_element_reads`), so its elements stay unboxed.
            let boxed = matches!(loan_ty, DirectType::Opaque(ty)
                if !matches!(ty, Type::Named(name, _) if name == "str"));
            return Ok(boxed.then_some((LoanTarget::Slot, None)));
        }
        let segments = projection.split('.').collect::<Vec<_>>();
        let Some((_, slot)) =
            self.class_slot(&DirectType::Opaque(element.clone()), &segments, loan_ty)?
        else {
            return Ok(None);
        };
        Ok(match (&slot.kind, lends) {
            (SlotKind::Leaves(leaves), false) => Some((
                LoanTarget::Place(LoanPlace::Leaves(leaves.clone())),
                Some(slot),
            )),
            (SlotKind::Handle, true) => Some((LoanTarget::Slot, Some(slot))),
            _ => None,
        })
    }

    /// Checks the one selection of `loan`'s view in the runtime, as the
    /// loan's beginning does when generated code cannot follow it. A loan
    /// generated code follows and that missed `promotes`: the runtime also
    /// counts the miss, and moves a list that misses often enough into
    /// boxes of its own (see `aura_direct_element_path_promote`).
    pub(super) fn emit_element_path_probe(
        &mut self,
        loan: &str,
        promotes: bool,
    ) -> std::result::Result<(), String> {
        let resolved = self.resolve_view_place(loan)?;
        let [alternative] = resolved.alternatives.as_slice() else {
            return Err(format!(
                "direct backend probed loan `{loan}` with more than one alternative"
            ));
        };
        let (collection, projection) = self.load_element_path_root(&alternative.place)?;
        let path = self.element_path_operands(&projection, &alternative.elements)?;
        let probe = if promotes {
            RuntimeFn::ElementPathPromote
        } else {
            RuntimeFn::ElementPathProbe
        };
        self.builder.call(
            self.runtime[probe],
            &[
                collection.values[0],
                path.path_ptr,
                path.path_len,
                path.selectors_ptr,
                path.count,
                path.line,
                path.column,
            ],
        );
        Ok(())
    }

    /// Lowers `ReadLoan` of a followed loan of a scalar or plain-record
    /// place: a load where the place lies, or the runtime read when the
    /// loan missed. Returns false for any other loan.
    pub(super) fn read_native_loan(
        &mut self,
        target: &str,
        loan: &str,
    ) -> std::result::Result<bool, String> {
        let Some(NativeLoan {
            address,
            target: LoanTarget::Place(place),
        }) = self.native_loans.get(loan).cloned()
        else {
            return Ok(false);
        };
        let loan_ty = self.local_type(loan)?;
        let target_ty = self.type_of_place(target)?;
        let fast_block = self.builder.create_block();
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        for abi in loan_ty.abi_types() {
            self.builder.append_block_param(join_block, abi);
        }
        let address = self.builder.use_var(address);
        self.builder.brif(address, fast_block, &[], slow_block, &[]);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let values = match &place {
            LoanPlace::Scalar(kind) => {
                let bits = self.load_scalar_list_element(*kind, address);
                vec![if kind.is_float() {
                    self.builder.bitcast(types::F64, bits)
                } else {
                    bits
                }]
            }
            LoanPlace::Leaves(leaves) => leaves
                .iter()
                .map(|leaf| self.load_leaf(*leaf, address))
                .collect(),
            LoanPlace::MapValue => {
                self.builder
                    .call(self.runtime[RuntimeFn::ValueSlotInt64], &[address])
                    .results
            }
        };
        self.builder.jump(join_block, &values);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let loaded = self.load_place(loan)?;
        let loaded = self.coerce_value(loaded, &loan_ty)?;
        self.release_temporary_owned_since(&caller_owned);
        self.builder.jump(join_block, &loaded.values);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let value = ValueRef {
            values: self.builder.block_params(join_block).to_vec(),
            ty: loan_ty,
        };
        let value = self.coerce_value(value, &target_ty)?;
        self.store_place(target, value)?;
        Ok(true)
    }

    /// Lowers `WriteLoan` of a followed loan of a scalar or plain-record
    /// place: a store where the place lies, or the runtime write when the
    /// loan missed. Returns false for any other loan.
    pub(super) fn write_native_loan(
        &mut self,
        loan: &str,
        value: &Rvalue,
    ) -> std::result::Result<bool, String> {
        let Some(NativeLoan {
            address,
            target: LoanTarget::Place(place),
        }) = self.native_loans.get(loan).cloned()
        else {
            return Ok(false);
        };
        let loan_ty = self.type_of_place(loan)?;
        let compiled = self.compile_rvalue_for_target(value, &loan_ty)?;
        let value = self.coerce_value(compiled, &loan_ty)?;
        let fast_block = self.builder.create_block();
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        let address = self.builder.use_var(address);
        self.builder.brif(address, fast_block, &[], slow_block, &[]);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        match &place {
            LoanPlace::Scalar(kind) => {
                let word = value.values[0];
                let bits = if kind.is_float() {
                    self.builder.bitcast(types::I64, word)
                } else {
                    word
                };
                self.store_scalar_list_element(*kind, bits, address);
            }
            LoanPlace::Leaves(leaves) => {
                for (leaf, word) in leaves.iter().zip(&value.values) {
                    self.store_leaf(*leaf, *word, address);
                }
            }
            LoanPlace::MapValue => {
                self.builder.call(
                    self.runtime[RuntimeFn::ValueSlotSetInt64],
                    &[address, value.values[0]],
                );
            }
        }
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        self.store_place(loan, value)?;
        self.release_temporary_owned_since(&caller_owned);
        self.builder.jump(join_block, &[]);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(true)
    }

    /// Lowers MIR `Lend` of a followed loan of a box slot to one runtime
    /// operation: the box in the slot is lent as it is, and `EndLend` ends
    /// the lend as it ends a field lend (see
    /// `classes::end_class_field_lend`). A loan that missed takes the
    /// element out as before. Returns false for any other lend.
    pub(super) fn lend_native_element(
        &mut self,
        place: &str,
        temp: &str,
        mutable: bool,
    ) -> std::result::Result<bool, String> {
        let address = match self.native_loans.get(place) {
            Some(NativeLoan {
                address,
                target: LoanTarget::Slot,
            }) if self.element_lend_temps.contains(temp) => *address,
            _ => return Ok(false),
        };
        let lent = self.declare_word_var();
        let slot_var = self.declare_word_var();
        let temp_var = self.local_vars(temp)?[0];
        self.lent_temps.insert(temp.to_string(), mutable);
        let fast_block = self.builder.create_block();
        let slow_block = self.builder.create_block();
        let lent_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        let address = self.builder.use_var(address);
        self.builder.brif(address, fast_block, &[], slow_block, &[]);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let boxed = self
            .builder
            .load(types::I64, MemFlags::trusted(), address, 0);
        // A moved-out field takes the runtime take, which reports it.
        self.builder.brif(boxed, lent_block, &[], slow_block, &[]);

        self.builder.switch_to_block(lent_block);
        self.builder.seal_block(lent_block);
        self.builder.def_var(temp_var, boxed);
        let one = self.builder.iconst(types::I64, 1);
        self.builder.def_var(lent, one);
        self.builder.def_var(slot_var, address);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let value = self.take_lent_place(place)?;
        self.mark_emptied_store_root(temp);
        self.store_root(temp, value)?;
        self.release_temporary_owned_since(&caller_owned);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(lent, zero);
        self.builder.def_var(slot_var, zero);
        self.builder.jump(join_block, &[]);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.field_lends.insert(
            temp.to_string(),
            FieldLend {
                lent,
                slot: slot_var,
            },
        );
        Ok(true)
    }

    /// Loads `place`, an argument to a parameter the callee borrows, as the
    /// box in the slot a followed loan reaches when `place` is such a loan:
    /// the callee reads the element where the list keeps it. A loan that
    /// missed, or a moved-out slot, loads the place as before, and the copy
    /// is released after the call as any temporary is. Returns `None` for
    /// any other place.
    pub(super) fn pass_native_element(
        &mut self,
        place: &str,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let address = match self.native_loans.get(place) {
            Some(NativeLoan {
                address,
                target: LoanTarget::Slot,
            }) => *address,
            _ => return Ok(None),
        };
        let loan_ty = self.local_type(place)?;
        if !matches!(loan_ty, DirectType::Opaque(_)) {
            return Ok(None);
        }
        let fast_block = self.builder.create_block();
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.builder.append_block_param(join_block, types::I64);
        let address = self.builder.use_var(address);
        self.builder.brif(address, fast_block, &[], slow_block, &[]);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let boxed = self
            .builder
            .load(types::I64, MemFlags::trusted(), address, 0);
        let lent_block = self.builder.create_block();
        // A moved-out field takes the runtime walk, which reports it.
        self.builder.brif(boxed, lent_block, &[], slow_block, &[]);

        self.builder.switch_to_block(lent_block);
        self.builder.seal_block(lent_block);
        let none = self.builder.iconst(types::I64, 0);
        self.builder.jump(join_block, &[boxed, none]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let loaded = self.load_place(place)?;
        let loaded = self.coerce_value(loaded, &loan_ty)?;
        let [word] = loaded.values.as_slice() else {
            return Err(format!(
                "direct backend loaded element loan `{place}` as more than one word"
            ));
        };
        let word = *word;
        let owned = self.layout_scratch.opaque.contains(&word);
        self.layout_scratch.remove(&word);
        self.release_temporary_owned_since(&caller_owned);
        let owned = if owned {
            word
        } else {
            self.builder.iconst(types::I64, 0)
        };
        self.builder.jump(join_block, &[word, owned]);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let [value, owned] = self.builder.block_params(join_block)[..] else {
            unreachable!("the join block takes two words");
        };
        // The copy a missed loan made, or 0, released once the call is done.
        self.layout_scratch.opaque.insert(owned);
        Ok(Some(ValueRef {
            values: vec![value],
            ty: loan_ty,
        }))
    }
}
