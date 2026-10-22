//! Lending (Batch 7 step 8e).
//!
//! A call that reads part of a value without keeping it, such as
//! `total(self.docs)` or `self.docs.len()`, would otherwise copy that part
//! out of its owner. The lending pass moves the part into a temporary for
//! the call and back after it:
//!
//! ```text
//! Lend { place: self.docs, temp: %t4, mutable: false }
//! %t0 = total(%t4)
//! EndLend { place: self.docs, temp: %t4, mutable: false }
//! ```
//!
//! The slot is empty while the call runs, so the pass lends a place only
//! when the call cannot reach it another way:
//!
//! - no other operand of the call, and not its target, overlaps the place;
//! - no operand reaches into a different root through a field or an
//!   element, because two borrowed parameters can name one instance;
//! - no argument's type can hold a class instance or a callable, which
//!   could alias the place's owner.
//!
//! A mutable lend serves a `mut` parameter or receiver whose writeback is
//! the lent place: the call writes the temporary back, and `EndLend`
//! publishes the change.
//!
//! The pass runs in the ownership pass, before the retains and releases
//! are planned, so they cover the temporaries too. The loan validator runs
//! [`validate_lends`], which accepts a lend only where the pass would make
//! it.

use std::collections::{HashMap, HashSet};

use super::ownership::{root_of, type_needs_drop};
use super::{
    mir_place_paths_overlap, substitute_type, BasicBlock, CallTarget, Instruction, MirArg,
    MirFunction, MirLoanValidationContext, MirLocalType, MirReceiverKind, Operand, Rvalue,
};
use crate::runtime_ops::{DictOp, ListOp, RuntimeOp, SetOp};
use crate::sema::Type;

/// Where reads of a place land: the place outside every loan, and whether
/// they reach it through a collection element.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Resolved {
    place: String,
    elements: bool,
}

/// The place each loan of a function borrows from. A loan begun more than
/// once, or one a call returns, has none, so nothing it reaches is lent.
struct LoanSources<'f> {
    begun: HashMap<&'f str, Vec<&'f Instruction>>,
    resolved: HashMap<&'f str, Option<Resolved>>,
}

impl<'f> LoanSources<'f> {
    fn of(function: &'f MirFunction) -> Self {
        let mut begun = HashMap::<&str, Vec<&Instruction>>::new();
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let loan = match instruction {
                Instruction::BeginLoan { loan, .. }
                | Instruction::BeginElementLoan { loan, .. }
                | Instruction::BeginReturnedLoan { loan, .. }
                | Instruction::Reborrow { loan, .. }
                | Instruction::ReborrowElement { loan, .. } => loan.as_str(),
                _ => continue,
            };
            begun.entry(loan).or_default().push(instruction);
        }
        let mut sources = Self {
            begun,
            resolved: HashMap::new(),
        };
        let loans = sources.begun.keys().copied().collect::<Vec<_>>();
        for loan in loans {
            sources.resolve_loan(loan, &mut HashSet::new());
        }
        sources
    }

    fn resolve_loan(&mut self, loan: &'f str, visiting: &mut HashSet<&'f str>) -> Option<Resolved> {
        if let Some(resolved) = self.resolved.get(loan) {
            return resolved.clone();
        }
        // A loan chain that leads back to itself names no place.
        if !visiting.insert(loan) {
            return None;
        }
        let resolved = match self.begun.get(loan).map(Vec::as_slice) {
            Some([instruction]) => match instruction {
                Instruction::BeginLoan { source, .. } => self.resolve_with(source, visiting),
                Instruction::BeginElementLoan { source, .. }
                | Instruction::ReborrowElement { parent: source, .. } => self
                    .resolve_with(source, visiting)
                    .map(|resolved| Resolved {
                        place: resolved.place,
                        elements: true,
                    }),
                Instruction::Reborrow {
                    parent, projection, ..
                } => self.resolve_with(parent, visiting).map(|resolved| {
                    if resolved.elements || projection.is_empty() {
                        resolved
                    } else {
                        Resolved {
                            place: format!("{}.{projection}", resolved.place),
                            elements: false,
                        }
                    }
                }),
                _ => None,
            },
            _ => None,
        };
        self.resolved.insert(loan, resolved.clone());
        resolved
    }

    fn resolve_with(
        &mut self,
        place: &'f str,
        visiting: &mut HashSet<&'f str>,
    ) -> Option<Resolved> {
        let root = root_of(place);
        if !self.begun.contains_key(root) {
            return Some(Resolved {
                place: place.to_string(),
                elements: false,
            });
        }
        let source = self.resolve_loan(root, visiting)?;
        Some(extend(source, &place[root.len()..]))
    }

    /// Where reads of `place` land, or `None` when a loan on the way names
    /// no single place.
    fn resolve(&self, place: &str) -> Option<Resolved> {
        let root = root_of(place);
        if !self.begun.contains_key(root) {
            return Some(Resolved {
                place: place.to_string(),
                elements: false,
            });
        }
        let source = self.resolved.get(root).cloned().flatten()?;
        Some(extend(source, &place[root.len()..]))
    }
}

/// `source` followed by the projection `rest`, which starts with a dot or
/// is empty. A projection inside a collection element stays in the
/// element, so the place stays the collection's.
fn extend(source: Resolved, rest: &str) -> Resolved {
    if source.elements || rest.is_empty() {
        source
    } else {
        Resolved {
            place: format!("{}{rest}", source.place),
            elements: false,
        }
    }
}

/// A place that names fields only: no union or enum payload projection,
/// which this pass cannot move out by name.
fn plain_path(place: &str) -> bool {
    !place.split('.').any(|segment| segment.starts_with("__"))
}

/// Whether reading `other` during the call could reach the slot of `lent`:
/// one place contains the other, or `other` reaches into a different root
/// through a field or an element. A whole other root, such as `ys`, holds
/// its own value.
fn overlaps(lent: &Resolved, other: &Resolved) -> bool {
    mir_place_paths_overlap(&lent.place, &other.place)
        || (root_of(&lent.place) != root_of(&other.place)
            && (other.place.contains('.') || other.elements))
}

/// Whether a value of `ty` can hold a handle to a class instance or a
/// callable, and so might reach a lent place's owner.
fn may_reach_instance(context: &MirLoanValidationContext<'_>, ty: &Type, depth: usize) -> bool {
    // A type nested this deep is treated as reaching one.
    if depth > 32 {
        return true;
    }
    match ty {
        Type::Named(name, args) => {
            if context.classes.contains_key(name.as_str())
                || args
                    .iter()
                    .any(|arg| may_reach_instance(context, arg, depth + 1))
            {
                return true;
            }
            let Some(enum_decl) = context.enums.get(name.as_str()) else {
                return false;
            };
            let substitutions = enum_decl
                .type_params
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect::<HashMap<_, _>>();
            enum_decl
                .variants
                .iter()
                .flat_map(|variant| &variant.payloads)
                .any(|payload| {
                    may_reach_instance(
                        context,
                        &substitute_type(payload, &substitutions),
                        depth + 1,
                    )
                })
        }
        Type::Tuple(items) => items
            .iter()
            .any(|item| may_reach_instance(context, item, depth + 1)),
        Type::Union(union) => union
            .members
            .iter()
            .any(|member| may_reach_instance(context, member, depth + 1)),
        Type::Unit | Type::Module(_) => false,
        _ => true,
    }
}

/// The operand of a call a lend takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LendSlot {
    Receiver,
    Arg(usize),
}

/// One lend the pass makes for a call.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PlannedLend {
    slot: LendSlot,
    place: String,
    mutable: bool,
}

/// The call an instruction makes, and the place it assigns.
fn call_of(instruction: &Instruction) -> Option<(&str, &CallTarget, &[MirArg])> {
    match instruction {
        Instruction::Assign {
            target,
            value: Rvalue::Call { callee, args },
        }
        | Instruction::WriteLoan {
            loan: target,
            value: Rvalue::Call { callee, args },
        } => Some((target, callee, args)),
        _ => None,
    }
}

fn call_of_mut(instruction: &mut Instruction) -> Option<(&mut CallTarget, &mut Vec<MirArg>)> {
    match instruction {
        Instruction::Assign {
            value: Rvalue::Call { callee, args },
            ..
        }
        | Instruction::WriteLoan {
            value: Rvalue::Call { callee, args },
            ..
        } => Some((callee, args)),
        _ => None,
    }
}

struct Planner<'f, 'c> {
    function: &'f MirFunction,
    context: &'c MirLoanValidationContext<'c>,
    sources: LoanSources<'f>,
}

impl Planner<'_, '_> {
    fn place_type(&self, place: &str) -> Option<Type> {
        self.context.place_type(self.function, place).ok().flatten()
    }

    /// The lends the pass makes for `instruction`, in operand order.
    fn plan(&self, instruction: &Instruction) -> Vec<PlannedLend> {
        let Some((target, callee, args)) = call_of(instruction) else {
            return Vec::new();
        };
        let mut lends = match callee {
            CallTarget::Name(name) => self.plan_args(name, args),
            CallTarget::Member { .. } | CallTarget::Runtime { .. } => {
                self.plan_receiver(callee, args).into_iter().collect()
            }
            CallTarget::Value(_) | CallTarget::TraitMember { .. } | CallTarget::Extern(_) => {
                Vec::new()
            }
        };
        // The call's result is stored before the lend ends, so a target
        // that reaches the slot would be overwritten by `EndLend`.
        let Some(target) = self.sources.resolve(target) else {
            return Vec::new();
        };
        lends.retain(|lend| {
            self.sources
                .resolve(&lend.place)
                .is_some_and(|lent| !overlaps(&lent, &target))
        });
        lends
    }

    /// The receiver lend of a member call or a runtime operation: a
    /// `list`, `dict`, or `set` field of a class instance, or an element a
    /// loan selects, given to a builtin operation; or an element a loan
    /// selects given to a class method.
    fn plan_receiver(&self, callee: &CallTarget, args: &[MirArg]) -> Option<PlannedLend> {
        let parts = callee.member_parts()?;
        let Operand::Place(place) = parts.object else {
            return None;
        };
        if !plain_path(place) || parts.receiver_place.is_some_and(|wb| wb != place) {
            return None;
        }
        let resolved = self.sources.resolve(place)?;
        if !plain_path(&resolved.place) {
            return None;
        }
        let root_ty = self.place_type(root_of(&resolved.place))?;
        if !matches!(root_ty, Type::Named(..)) {
            return None;
        }
        if !resolved.elements {
            // A field place: the owner must be a class instance, so the
            // field moves out of it by name.
            let Type::Named(owner, _) = &root_ty else {
                return None;
            };
            if !self.context.classes.contains_key(owner.as_str()) || !resolved.place.contains('.') {
                return None;
            }
        }
        let Type::Named(receiver, _) = self.place_type(place)? else {
            return None;
        };
        // A receiver the call may change must come back through a
        // writeback, which the lend then makes mutable.
        let writes_back = parts.receiver_place.is_some();
        let passing = if matches!(receiver.as_str(), "list" | "dict" | "set") {
            // A consuming read or a span op stores the receiver back itself.
            let CallTarget::Runtime { op, .. } = callee else {
                return None;
            };
            if matches!(
                op,
                RuntimeOp::List(ListOp::TakeIndexOption)
                    | RuntimeOp::Set(SetOp::TakeIndexOption)
                    | RuntimeOp::Span(_)
            ) {
                return None;
            }
            // The size of an element of a local list, or a lookup in it, is
            // read where the element lies, so it needs no lend.
            if resolved.elements
                && !resolved.place.contains('.')
                && matches!(
                    op,
                    RuntimeOp::List(ListOp::Len)
                        | RuntimeOp::Dict(DictOp::Len | DictOp::Get | DictOp::GetStr)
                        | RuntimeOp::Set(SetOp::Len)
                )
            {
                return None;
            }
            // An op with no declared passing, such as an element read, may
            // change a receiver it writes back when its owner allows it.
            let declared = self
                .context
                .member_receiver_passing(self.function, parts.object, None, parts.field)
                .ok()
                .flatten();
            declared.unwrap_or(
                if writes_back
                    && !resolved.elements
                    && self
                        .context
                        .root_allows_mutable_loan(self.function, root_of(&resolved.place))
                {
                    MirReceiverKind::BorrowMut
                } else {
                    MirReceiverKind::Borrow
                },
            )
        } else if resolved.elements {
            let CallTarget::Member { field, .. } = callee else {
                return None;
            };
            self.context
                .classes
                .get(receiver.as_str())?
                .methods
                .iter()
                .find(|method| &method.name == field)?
                .receiver?
        } else {
            return None;
        };
        let mutable = match passing {
            MirReceiverKind::Borrow => false,
            MirReceiverKind::BorrowMut if writes_back => true,
            _ => return None,
        };
        for arg in args {
            if arg.writeback_place.is_some() {
                return None;
            }
            let (Operand::Place(other) | Operand::MovePlace(other)) = &arg.value else {
                continue;
            };
            if overlaps(&resolved, &self.sources.resolve(other)?)
                || may_reach_instance(self.context, &self.place_type(other)?, 0)
            {
                return None;
            }
        }
        Some(PlannedLend {
            slot: LendSlot::Receiver,
            place: place.clone(),
            mutable,
        })
    }

    /// The argument lends of a direct call: a field of a class instance
    /// given to a shared parameter, or to a `mut` parameter whose writeback
    /// is the field.
    fn plan_args(&self, name: &str, args: &[MirArg]) -> Vec<PlannedLend> {
        let Some(callee) = self.context.functions.get(name) else {
            return Vec::new();
        };
        let offset = usize::from(callee.receiver.is_some());
        let passing = |position: usize, arg: &MirArg| match &arg.name {
            Some(name) => callee
                .params
                .iter()
                .find(|param| &param.name == name)
                .map(|param| param.passing),
            None if position < offset => callee.receiver,
            None => callee
                .params
                .get(position - offset)
                .map(|param| param.passing),
        };
        let mut resolved = Vec::with_capacity(args.len());
        for arg in args {
            let (Operand::Place(place) | Operand::MovePlace(place)) = &arg.value else {
                resolved.push(None);
                continue;
            };
            // Nothing is lent when an argument could reach the owner.
            let (Some(place_resolved), Some(ty)) =
                (self.sources.resolve(place), self.place_type(place))
            else {
                return Vec::new();
            };
            if may_reach_instance(self.context, &ty, 0) {
                return Vec::new();
            }
            resolved.push(Some((place_resolved, ty)));
        }
        let mut lends = Vec::new();
        for (position, arg) in args.iter().enumerate() {
            let Operand::Place(place) = &arg.value else {
                continue;
            };
            let mutable = match (passing(position, arg), &arg.writeback_place) {
                (Some(MirReceiverKind::Borrow), None) => false,
                (Some(MirReceiverKind::BorrowMut), Some(writeback)) if writeback == place => true,
                _ => continue,
            };
            let Some((lent, ty)) = &resolved[position] else {
                continue;
            };
            if lent.elements
                || !plain_path(&lent.place)
                || !lent.place.contains('.')
                || !lends_by_handle(self.context, ty)
            {
                continue;
            }
            let owner_is_instance = matches!(
                self.place_type(root_of(&lent.place)),
                Some(Type::Named(owner, _)) if self.context.classes.contains_key(owner.as_str())
            );
            let overlapped = resolved.iter().enumerate().any(|(other, entry)| {
                other != position
                    && entry
                        .as_ref()
                        .is_some_and(|(candidate, _)| overlaps(lent, candidate))
            });
            if owner_is_instance && !overlapped {
                lends.push(PlannedLend {
                    slot: LendSlot::Arg(position),
                    place: place.clone(),
                    mutable,
                });
            }
        }
        lends
    }
}

/// Whether a backend passes a value of `ty` as one handle that a lend can
/// move: a value that needs drop and is neither a union nor a callable.
fn lends_by_handle(context: &MirLoanValidationContext<'_>, ty: &Type) -> bool {
    matches!(ty, Type::Named(..) | Type::Tuple(_)) && type_needs_drop(context, ty)
}

/// Points the slot of `lend` at `temp`, with its writeback.
fn redirect(instruction: &mut Instruction, lend: &PlannedLend, temp: &str) {
    let Some((callee, args)) = call_of_mut(instruction) else {
        return;
    };
    let (operand, writeback) = match lend.slot {
        LendSlot::Receiver => match callee {
            CallTarget::Member {
                object,
                receiver_place,
                ..
            }
            | CallTarget::Runtime {
                receiver: Some(object),
                receiver_place,
                ..
            } => (object, receiver_place),
            _ => return,
        },
        LendSlot::Arg(position) => {
            let Some(arg) = args.get_mut(position) else {
                return;
            };
            (&mut arg.value, &mut arg.writeback_place)
        }
    };
    *operand = Operand::Place(temp.to_string());
    if writeback.is_some() {
        *writeback = Some(temp.to_string());
    }
}

/// `function` with its lends, or `None` when it makes none. The new
/// temporaries follow the function's `%tN` locals.
pub(super) fn lend_function(
    function: &MirFunction,
    context: &MirLoanValidationContext<'_>,
) -> Option<MirFunction> {
    let planner = Planner {
        function,
        context,
        sources: LoanSources::of(function),
    };
    let mut next_temporary = function
        .local_types
        .iter()
        .filter_map(|local| local.name.strip_prefix("%t")?.parse::<usize>().ok())
        .max()
        .map_or(0, |max| max + 1);
    let mut locals = Vec::new();
    let mut blocks = Vec::with_capacity(function.blocks.len());
    for block in &function.blocks {
        let mut instructions = Vec::with_capacity(block.instructions.len());
        for (position, instruction) in block.instructions.iter().enumerate() {
            // A returned view follows its call at once. The lends of an
            // element read an `IndexInRange` covers go before the fact,
            // which then names the temporary.
            let pinned = matches!(instruction, Instruction::IndexInRange { .. })
                || matches!(
                    block.instructions.get(position + 1),
                    Some(Instruction::BeginReturnedLoan { .. })
                );
            let lends = if pinned {
                Vec::new()
            } else {
                planner.plan(instruction)
            };
            if lends.is_empty() {
                instructions.push(instruction.clone());
                continue;
            }
            let covered = (position > 0
                && matches!(
                    block.instructions[position - 1],
                    Instruction::IndexInRange { .. }
                ))
            .then(|| instructions.pop())
            .flatten();
            let mut call = instruction.clone();
            let mut ends = Vec::with_capacity(lends.len());
            for lend in &lends {
                let Some(ty) = planner.place_type(&lend.place) else {
                    continue;
                };
                let temp = format!("%t{next_temporary}");
                next_temporary += 1;
                locals.push(MirLocalType {
                    name: temp.clone(),
                    ty,
                });
                instructions.push(Instruction::Lend {
                    place: lend.place.clone(),
                    temp: temp.clone(),
                    mutable: lend.mutable,
                });
                redirect(&mut call, lend, &temp);
                ends.push(Instruction::EndLend {
                    place: lend.place.clone(),
                    temp,
                    mutable: lend.mutable,
                });
            }
            if let Some(mut fact) = covered {
                if let Instruction::IndexInRange { list, .. } = &mut fact {
                    if let Some(Instruction::EndLend { temp, .. }) = ends.iter().find(
                        |end| matches!(end, Instruction::EndLend { place, .. } if place == list),
                    ) {
                        *list = temp.clone();
                    }
                }
                instructions.push(fact);
            }
            instructions.push(call);
            instructions.extend(ends.into_iter().rev());
        }
        blocks.push(BasicBlock {
            label: block.label.clone(),
            instructions,
            terminator: block.terminator.clone(),
        });
    }
    if locals.is_empty() {
        return None;
    }
    let mut lent = function.clone();
    lent.blocks = blocks;
    lent.local_types.extend(locals);
    lent.local_types
        .sort_by(|left, right| left.name.cmp(&right.name));
    Some(lent)
}

/// One `Lend` of a call: its place, temporary, and mutability.
type ActiveLend<'f> = (&'f str, &'f str, bool);

/// `call` as it was before the pass lent `lends`: each temporary is the
/// place again.
fn unlent_call(call: &Instruction, lends: &[ActiveLend<'_>]) -> Instruction {
    let mut call = call.clone();
    let back = |name: &mut String| {
        if let Some((place, _, _)) = lends.iter().find(|(_, temp, _)| *temp == name.as_str()) {
            *name = (*place).to_string();
        }
    };
    if let Some((callee, args)) = call_of_mut(&mut call) {
        let receiver = match callee {
            CallTarget::Member {
                object,
                receiver_place,
                ..
            }
            | CallTarget::Runtime {
                receiver: Some(object),
                receiver_place,
                ..
            } => Some((object, receiver_place)),
            _ => None,
        };
        let slots = receiver.into_iter().chain(
            args.iter_mut()
                .map(|arg| (&mut arg.value, &mut arg.writeback_place)),
        );
        for (operand, writeback) in slots {
            if let Operand::Place(name) = operand {
                back(name);
            }
            if let Some(name) = writeback {
                back(name);
            }
        }
    }
    call
}

/// Checks every lend of `function`. A lend brackets one call in its
/// block: `Lend`s, retains, the call, releases, then the `EndLend`s in
/// reverse order. Its temporary is a fresh `%tN` local the block names
/// nowhere else, and the call must be one the pass would lend exactly
/// these places for.
pub(super) fn validate_lends(
    function: &MirFunction,
    context: &MirLoanValidationContext<'_>,
) -> Result<(), String> {
    let lends_anything = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(
                instruction,
                Instruction::Lend { .. } | Instruction::EndLend { .. }
            )
        });
    if !lends_anything {
        return Ok(());
    }
    let invalid = |message: &str| format!("invalid MIR in `{}` {message}", function.name);
    let mut planner: Option<Planner<'_, '_>> = None;
    let mut temps = HashSet::new();
    for block in &function.blocks {
        let instructions = &block.instructions;
        let mut position = 0;
        while position < instructions.len() {
            if matches!(instructions[position], Instruction::EndLend { .. }) {
                return Err(invalid("ends a lend it never began"));
            }
            if !matches!(instructions[position], Instruction::Lend { .. }) {
                position += 1;
                continue;
            }
            let mut lends = Vec::<ActiveLend<'_>>::new();
            while let Some(instruction) = instructions.get(position) {
                match instruction {
                    Instruction::Lend {
                        place,
                        temp,
                        mutable,
                    } => lends.push((place, temp, *mutable)),
                    Instruction::Retain { .. } | Instruction::IndexInRange { .. } => {}
                    _ => break,
                }
                position += 1;
            }
            let Some(call) = instructions
                .get(position)
                .filter(|call| call_of(call).is_some())
            else {
                return Err(invalid("lends a place to something other than a call"));
            };
            position += 1;
            while matches!(
                instructions.get(position),
                Some(Instruction::Release { .. })
            ) {
                position += 1;
            }
            for &(place, temp, mutable) in lends.iter().rev() {
                match instructions.get(position) {
                    Some(Instruction::EndLend {
                        place: end_place,
                        temp: end_temp,
                        mutable: end_mutable,
                    }) if end_place == place && end_temp == temp && *end_mutable == mutable => {}
                    _ => {
                        return Err(invalid(&format!(
                            "does not end the lend of `{place}` after its call"
                        )))
                    }
                }
                position += 1;
            }
            for &(_, temp, _) in &lends {
                let fresh = temp
                    .strip_prefix("%t")
                    .is_some_and(|number| number.parse::<usize>().is_ok())
                    && function.local_types.iter().any(|local| local.name == temp)
                    && !function.params.iter().any(|param| param.name == temp)
                    && temps.insert(temp);
                if !fresh {
                    return Err(invalid(&format!(
                        "lends into `{temp}`, which is not a fresh temporary"
                    )));
                }
            }
            let planner = planner.get_or_insert_with(|| Planner {
                function,
                context,
                sources: LoanSources::of(function),
            });
            let expected = planner.plan(&unlent_call(call, &lends));
            let matches = expected.len() == lends.len()
                && expected
                    .iter()
                    .zip(&lends)
                    .all(|(planned, (place, _, mutable))| {
                        planned.place == *place && planned.mutable == *mutable
                    });
            if !matches {
                return Err(invalid("lends a place the call could reach another way"));
            }
        }
    }
    // A temporary is named only by its lend, the call, and the retains and
    // releases around them.
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        let mut mentions = Vec::new();
        super::instruction_place_mentions(instruction, &mut mentions);
        let lend = matches!(
            instruction,
            Instruction::Lend { .. }
                | Instruction::EndLend { .. }
                | Instruction::Retain { .. }
                | Instruction::Release { .. }
                | Instruction::IndexInRange { .. }
        ) || call_of(instruction).is_some();
        if !lend && mentions.iter().any(|place| temps.contains(root_of(place))) {
            return Err(invalid("uses a lent temporary outside its call"));
        }
    }
    Ok(())
}

/// Undoes the lends of `function`: each call reads its places again, and
/// the temporaries go. Tests use it to forge implicit MIR.
pub(super) fn unlend_function(function: &mut MirFunction) {
    let mut temps = HashSet::new();
    for block in &mut function.blocks {
        let mut instructions = Vec::with_capacity(block.instructions.len());
        let mut pending = Vec::<(String, String, bool)>::new();
        for instruction in std::mem::take(&mut block.instructions) {
            match instruction {
                Instruction::Lend {
                    place,
                    temp,
                    mutable,
                } => {
                    temps.insert(temp.clone());
                    pending.push((place, temp, mutable));
                }
                Instruction::EndLend { .. } => {}
                Instruction::IndexInRange { list, index } => {
                    let list = pending
                        .iter()
                        .find(|(_, temp, _)| *temp == list)
                        .map_or(list.clone(), |(place, _, _)| place.clone());
                    instructions.push(Instruction::IndexInRange { list, index });
                }
                instruction if call_of(&instruction).is_some() && !pending.is_empty() => {
                    let lends = pending
                        .iter()
                        .map(|(place, temp, mutable)| (place.as_str(), temp.as_str(), *mutable))
                        .collect::<Vec<_>>();
                    instructions.push(unlent_call(&instruction, &lends));
                    pending.clear();
                }
                instruction => instructions.push(instruction),
            }
        }
        block.instructions = instructions;
    }
    function
        .local_types
        .retain(|local| !temps.contains(&local.name));
}

#[cfg(test)]
#[path = "mir_lending_tests.rs"]
mod tests;
