//! The operand-role table of explicit ownership (Batch 7 step 8e).
//!
//! Each operand of an instruction or a terminator either consumes the value
//! it names or borrows it. A consuming `Place` operand takes one reference,
//! so an ownership pass retains the value first unless the operand is the
//! value's last use. A borrowing operand leaves the value where it is. A
//! `MovePlace` operand always moves, whatever its role.
//!
//! The ownership pass, the ownership checker, and both backends read this
//! one table.
//!
//! The ownership pass, [`insert_ownership`], states each reference a value
//! gains or loses. It retains a value before each consuming `Place` operand
//! and releases a root local after its last use, so every path releases each
//! value exactly once. `mir_ownership_check.rs` checks the result.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::{
    mir_successors, substitute_type, BasicBlock, CallTarget, CleanupCallee, Instruction,
    MethodDispatch, MirArg, MirFormatPart, MirFunction, MirLoanValidationContext, MirLocalType,
    MirModule, MirOwnership, MirReceiverKind, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::RuntimeOp;
use crate::sema::Type;

/// What an operand does with the value it names.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OperandRole {
    /// The operation takes one reference to the value.
    Consume,
    /// The operation reads the value and leaves it in place.
    Borrow,
}

impl OperandRole {
    /// The role of an argument passed with `passing`: an `own` parameter
    /// consumes, and a shared or mutable one borrows.
    pub fn of_passing(passing: crate::ast::ReceiverKind) -> Self {
        match passing {
            crate::ast::ReceiverKind::Value => OperandRole::Consume,
            crate::ast::ReceiverKind::Borrow | crate::ast::ReceiverKind::BorrowMut => {
                OperandRole::Borrow
            }
        }
    }
}

/// The receiver a call reads where it lies, though it names a method the
/// callee owns its receiver for: the `Place` object of a `Member` or
/// `TraitMember` call whose type, by `place_type`, is a type parameter, as
/// the call finds the method on the boxed value; or of a union's `clone`,
/// which copies the active member.
pub fn borrowed_receiver(
    instruction: &Instruction,
    place_type: impl Fn(&str) -> Option<Type>,
) -> Option<&Operand> {
    let (Instruction::Assign {
        value: Rvalue::Call { callee, args, .. },
        ..
    }
    | Instruction::WriteLoan {
        value: Rvalue::Call { callee, args, .. },
        ..
    }) = instruction
    else {
        return None;
    };
    let (clone, object) = match callee {
        CallTarget::Member { object, field, .. } => (field == "clone" && args.is_empty(), object),
        CallTarget::TraitMember { object, .. } => (false, object),
        _ => return None,
    };
    let Operand::Place(place) = object else {
        return None;
    };
    match place_type(place) {
        Some(Type::TypeParam(_)) => Some(object),
        Some(Type::Union(_)) if clone => Some(object),
        _ => None,
    }
}

/// [`operand_roles`] with each operand of `borrowed` borrowed: the
/// receiver [`borrowed_receiver`] finds, and the arguments
/// [`callee_borrowed_operands`] finds.
pub fn operand_roles_with<'a>(
    instruction: &'a Instruction,
    borrowed: &[&Operand],
) -> Vec<(&'a Operand, OperandRole)> {
    let mut roles = operand_roles(instruction);
    for (operand, role) in &mut roles {
        if borrowed
            .iter()
            .any(|borrowed| std::ptr::eq(*operand, *borrowed))
        {
            *role = OperandRole::Borrow;
        }
    }
    roles
}

/// The arguments, and the receiver, of `instruction`'s call that its
/// callee borrows where they lie. `borrowing` names a function whose shared
/// parameters and shared receiver the caller keeps (see
/// [`recorded_borrowing`]): the caller passes its own reference, and the
/// callee neither owns nor releases it.
pub fn callee_borrowed_operands<'a, 'm>(
    instruction: &'a Instruction,
    borrowing: impl Fn(&str) -> Option<&'m BorrowingCallee>,
) -> Vec<&'a Operand> {
    let (Instruction::Assign {
        value: Rvalue::Call { callee, args, .. },
        ..
    }
    | Instruction::WriteLoan {
        value: Rvalue::Call { callee, args, .. },
        ..
    }) = instruction
    else {
        return Vec::new();
    };
    let (function, receiver) = match callee {
        CallTarget::Name(name) => (borrowing(name), None),
        CallTarget::Member {
            object,
            dispatch: Some(MethodDispatch::Static { function }),
            ..
        }
        | CallTarget::TraitMember {
            object,
            dispatch: Some(MethodDispatch::Static { function }),
            ..
        } => (borrowing(function), Some(object)),
        _ => return Vec::new(),
    };
    let Some(function) = function else {
        return Vec::new();
    };
    if function.params.len() != args.len() || function.receiver.is_some() != receiver.is_some() {
        return Vec::new();
    }
    let mut borrowed = Vec::new();
    if function.receiver == Some(MirReceiverKind::Borrow) {
        borrowed.extend(receiver);
    }
    for (passing, arg) in function.params.iter().zip(args) {
        if *passing == MirReceiverKind::Borrow && arg.writeback_place.is_none() {
            borrowed.push(&arg.value);
        }
    }
    borrowed
}

/// How a borrowing function (see [`recorded_borrowing`]) takes its
/// receiver and each parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowingCallee {
    pub receiver: Option<MirReceiverKind>,
    pub params: Vec<MirReceiverKind>,
}

/// The functions a union arm or a runtime type picks for `dispatch`.
fn dispatched_functions<'d>(dispatch: &'d MethodDispatch, into: &mut BTreeSet<&'d str>) {
    match dispatch {
        MethodDispatch::Union(arms) => {
            for arm in arms {
                if let MethodDispatch::Static { function } = arm {
                    into.insert(function);
                }
                dispatched_functions(arm, into);
            }
        }
        MethodDispatch::Candidates { candidates, .. } => {
            into.extend(
                candidates
                    .iter()
                    .map(|candidate| candidate.function.as_str()),
            );
        }
        _ => {}
    }
}

/// Every operand of `instruction` its call borrows: the receiver of a call
/// on a boxed generic value, and the arguments a borrowing callee keeps
/// with the caller.
pub(super) fn borrowed_operands<'a>(
    instruction: &'a Instruction,
    context: &MirLoanValidationContext<'_>,
    place_type: impl Fn(&str) -> Option<Type>,
) -> Vec<&'a Operand> {
    let mut borrowed =
        callee_borrowed_operands(instruction, |name| context.borrowing_function(name));
    borrowed.extend(borrowed_receiver(instruction, place_type));
    borrowed
}

/// The functions whose shared parameters and shared receiver their callers
/// keep (Rust-speed step 2h): every function only named calls and method
/// calls MIR resolves to it reach. The language lets a shared parameter be
/// neither rebound nor mutably borrowed while the call runs (AU3002,
/// AU3003), so the callee can read the caller's value where it lies. Any
/// other way into a function keeps the owning convention: a function value
/// or task entry, a closure body, a cleanup or `From` callee, a method a
/// union or a runtime type picks, a module constant's initializer, and the
/// entry points. So does a function that returns a view into an argument.
///
/// Native emission drops the builtin wrappers a program never reaches, and
/// a dropped wrapper may have been the only other way into a function. So
/// the pass records the functions it chose in the module (see
/// [`recorded_borrowing`]), and the checker only asks that each recorded
/// function still be one of these.
pub fn borrowing_functions(module: &MirModule) -> BTreeSet<String> {
    let mut reached_otherwise = BTreeSet::<&str>::new();
    for constant in &module.constants {
        reached_otherwise.insert(&constant.initializer);
    }
    let functions = || module.functions.iter().chain(module.top_level.iter());
    for function in functions() {
        for block in &function.blocks {
            for instruction in &block.instructions {
                for (operand, _) in operand_roles(instruction) {
                    if let Operand::Function { name, .. } = operand {
                        reached_otherwise.insert(name);
                    }
                }
                match instruction {
                    Instruction::PushCleanup {
                        callee: Some(CleanupCallee::Method { function }),
                        ..
                    } => {
                        reached_otherwise.insert(function);
                    }
                    Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
                        match value {
                            Rvalue::Closure { function, .. } => {
                                reached_otherwise.insert(function);
                            }
                            Rvalue::Try {
                                from: Some(function),
                                ..
                            } => {
                                reached_otherwise.insert(function);
                            }
                            Rvalue::Call {
                                callee:
                                    CallTarget::Member {
                                        dispatch: Some(dispatch),
                                        ..
                                    }
                                    | CallTarget::TraitMember {
                                        dispatch: Some(dispatch),
                                        ..
                                    },
                                ..
                            } => dispatched_functions(dispatch, &mut reached_otherwise),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            for (operand, _) in terminator_roles(&block.terminator) {
                if let Operand::Function { name, .. } = operand {
                    reached_otherwise.insert(name);
                }
            }
        }
    }
    module
        .functions
        .iter()
        .filter(|function| {
            !matches!(function.name.as_str(), "main" | "__script")
                && !function.name.contains("::__default_")
                && !reached_otherwise.contains(function.name.as_str())
                && borrows_anything(function)
                && !returns_a_view(function)
        })
        .map(|function| function.name.clone())
        .collect()
}

/// Whether `function` returns a view into one of its arguments, which the
/// returned-loan rules tie to the owning convention.
fn returns_a_view(function: &MirFunction) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(instruction, Instruction::ReturnLoan { .. }))
}

fn borrows_anything(function: &MirFunction) -> bool {
    function.receiver == Some(MirReceiverKind::Borrow)
        || function
            .params
            .iter()
            .any(|param| param.passing == MirReceiverKind::Borrow)
}

/// The functions `module` records as keeping their callers' shared
/// parameters and shared receiver, with how each takes them. A recorded
/// name the module no longer defines is left out.
///
/// Only a value the module retains and releases stays with the caller. A
/// shared parameter whose type needs no drop, such as a `Queue` handle or
/// a Copy record, is one the module copies freely and a backend may still
/// box, so it keeps the owning convention and reads as `Value`.
pub(super) fn recorded_borrowing(
    module: &MirModule,
    context: &MirLoanValidationContext<'_>,
) -> BTreeMap<String, BorrowingCallee> {
    if module.borrowing.is_empty() {
        return BTreeMap::new();
    }
    let kept = |passing: MirReceiverKind, ty: Option<&Type>| match (passing, ty) {
        (MirReceiverKind::Borrow, Some(ty)) if type_needs_drop(context, ty) => {
            MirReceiverKind::Borrow
        }
        (MirReceiverKind::Borrow, _) => MirReceiverKind::Value,
        (passing, _) => passing,
    };
    module
        .functions
        .iter()
        .filter(|function| module.borrowing.contains(&function.name))
        .map(|function| {
            let receiver_type = function
                .local_types
                .iter()
                .find(|local| local.name == "self")
                .map(|local| &local.ty);
            let callee = BorrowingCallee {
                receiver: function
                    .receiver
                    .map(|passing| kept(passing, receiver_type)),
                params: function
                    .params
                    .iter()
                    .map(|param| kept(param.passing, Some(&param.ty)))
                    .collect(),
            };
            (function.name.clone(), callee)
        })
        .collect()
}

/// [`recorded_borrowing`] for a backend: how each function that keeps its
/// callers' shared arguments takes them, or nothing for a module without
/// explicit ownership.
pub(crate) fn kept_arguments(module: &MirModule) -> BTreeMap<String, BorrowingCallee> {
    if module.ownership != MirOwnership::Explicit {
        return BTreeMap::new();
    }
    MirLoanValidationContext::new(module)
        .map(|context| context.borrowing)
        .unwrap_or_default()
}

/// Every operand of `instruction` with its role, in evaluation order. A
/// call on a boxed generic receiver borrows it: see
/// [`operand_roles_with`].
pub fn operand_roles(instruction: &Instruction) -> Vec<(&Operand, OperandRole)> {
    let mut roles = Vec::new();
    match instruction {
        Instruction::Safepoint
        | Instruction::Retain { .. }
        | Instruction::Release { .. }
        | Instruction::Lend { .. }
        | Instruction::EndLend { .. }
        | Instruction::IndexInRange { .. }
        | Instruction::BeginLoan { .. }
        | Instruction::BeginReturnedLoan { .. }
        | Instruction::Reborrow { .. }
        | Instruction::ReadLoan { .. }
        | Instruction::EndLoan { .. }
        | Instruction::ReturnLoan { .. }
        | Instruction::PushCleanup { .. }
        | Instruction::PopCleanup { .. } => {}
        // The loan owns its selector until `EndLoan`.
        Instruction::BeginElementLoan { selector, .. }
        | Instruction::ReborrowElement { selector, .. } => {
            roles.push((selector, OperandRole::Consume))
        }
        Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
            rvalue_roles(value, &mut roles)
        }
        Instruction::Eval { value } => roles.push((value, OperandRole::Borrow)),
    }
    roles
}

/// Every operand of `terminator` with its role.
pub fn terminator_roles(terminator: &Terminator) -> Vec<(&Operand, OperandRole)> {
    match terminator {
        Terminator::Return(value) => vec![(value, OperandRole::Consume)],
        Terminator::Goto(_) | Terminator::Unreachable => Vec::new(),
        Terminator::Branch { condition, .. } => vec![(condition, OperandRole::Borrow)],
        Terminator::ForRange { iterable, .. } => vec![(iterable, OperandRole::Borrow)],
        Terminator::Match { scrutinee, .. } => vec![(scrutinee, OperandRole::Borrow)],
        // A failed assertion traps, and the task's ledger reclaims what it
        // owns.
        Terminator::AssertFail {
            message, captures, ..
        } => message
            .iter()
            .chain(captures.iter().map(|capture| &capture.value))
            .map(|operand| (operand, OperandRole::Borrow))
            .collect(),
    }
}

fn rvalue_roles<'a>(value: &'a Rvalue, roles: &mut Vec<(&'a Operand, OperandRole)>) {
    use OperandRole::{Borrow, Consume};
    match value {
        // The target takes the value. A callable adapter and a union
        // injection wrap it, and `try` unwraps it.
        Rvalue::Use(operand)
        | Rvalue::CallableAdapt { value: operand, .. }
        | Rvalue::UnionInject { value: operand, .. }
        | Rvalue::Try { value: operand, .. } => roles.push((operand, Consume)),
        // A projection's result is a fresh owned copy of the part it reads.
        Rvalue::NoneTest { value: operand }
        | Rvalue::Unary { value: operand, .. }
        | Rvalue::Cast { value: operand, .. }
        | Rvalue::TupleElement { tuple: operand, .. }
        | Rvalue::VariantPayload {
            scrutinee: operand, ..
        }
        | Rvalue::Member {
            object: operand, ..
        } => roles.push((operand, Borrow)),
        Rvalue::UnionTagTest { .. }
        | Rvalue::UnionTakePayload { .. }
        | Rvalue::TupleTakeElement { .. }
        | Rvalue::ModuleConstant { .. } => {}
        Rvalue::Closure { captures, .. } => {
            for capture in captures {
                let role = match capture.passing {
                    MirReceiverKind::Value => Consume,
                    MirReceiverKind::Borrow | MirReceiverKind::BorrowMut => Borrow,
                };
                roles.push((&capture.value, role));
            }
        }
        Rvalue::FormatString { parts } => {
            for operand in parts.iter().filter_map(MirFormatPart::operand) {
                roles.push((operand, Borrow));
            }
        }
        Rvalue::Binary { left, right, .. } => {
            roles.push((left, Borrow));
            roles.push((right, Borrow));
        }
        // The runtime copies a task's arguments and function across.
        Rvalue::StartTask {
            stack_size,
            task_group,
            function,
            args,
            ..
        } => {
            for operand in stack_size.iter().chain([task_group, function]) {
                roles.push((operand, Borrow));
            }
            for arg in args {
                roles.push((&arg.value, Borrow));
            }
        }
        Rvalue::Call { callee, args, .. } => call_roles(callee, args, roles),
        Rvalue::VecLiteral { elements, .. }
        | Rvalue::TupleLiteral { elements, .. }
        | Rvalue::SetLiteral { elements, .. } => {
            for element in elements {
                roles.push((element, Consume));
            }
        }
        Rvalue::MapLiteral { entries, .. } => {
            for entry in entries {
                roles.push((&entry.key, Consume));
                roles.push((&entry.value, Consume));
            }
        }
        Rvalue::Construct { fields, .. } => {
            for field in fields {
                roles.push((&field.value, Consume));
            }
        }
        Rvalue::EnumVariant { payloads, .. } => {
            for payload in payloads {
                roles.push((payload, Consume));
            }
        }
    }
}

fn call_roles<'a>(
    callee: &'a CallTarget,
    args: &'a [MirArg],
    roles: &mut Vec<(&'a Operand, OperandRole)>,
) {
    use OperandRole::{Borrow, Consume};
    match callee {
        // The callee owns every parameter, including its receiver.
        CallTarget::Name(_) => {}
        CallTarget::Value(function) => roles.push((function, Borrow)),
        CallTarget::Member { object, .. } | CallTarget::TraitMember { object, .. } => {
            roles.push((object, Consume))
        }
        // An extern call marshals copies of its arguments.
        CallTarget::Extern(_) => {
            for arg in args {
                roles.push((&arg.value, Borrow));
            }
            return;
        }
        CallTarget::Runtime { op, receiver, .. } => {
            if let (Some(receiver), Some(role)) = (receiver, op.receiver_ownership()) {
                roles.push((receiver, role));
            }
            for (position, arg) in args.iter().enumerate() {
                roles.push((&arg.value, runtime_arg_role(*op, position)));
            }
            return;
        }
    }
    for arg in args {
        roles.push((&arg.value, Consume));
    }
}

/// The role of a runtime op's argument: the ownership of the parameter at
/// its position, since MIR binds a resolved op's arguments in parameter
/// order. An argument the op does not declare borrows, as a variadic
/// `select` source does.
///
/// An op's parameter table is costly to build, and every pass and check
/// asks for it, so each thread keeps the answers it has found.
fn runtime_arg_role(op: RuntimeOp, position: usize) -> OperandRole {
    type Roles = HashMap<(RuntimeOp, usize), Option<OperandRole>>;
    thread_local! {
        static ROLES: RefCell<Roles> = RefCell::new(HashMap::new());
    }
    ROLES
        .with(|roles| {
            *roles
                .borrow_mut()
                .entry((op, position))
                .or_insert_with(|| op.arg_ownership(position))
        })
        .unwrap_or(OperandRole::Borrow)
}

/// The largest tracked-root count times reachable-block count either
/// ownership analysis keeps state for. The checker refuses a bigger
/// function, and the pass leaves a module that holds one implicit.
pub(super) const MAX_OWNERSHIP_STATE_CELLS: usize = 1 << 24;

/// A set of tracked roots, by index. Indexes follow name order, so a walk
/// over the set visits the roots in name order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct RootSet {
    words: Vec<u64>,
}

impl RootSet {
    pub(super) fn new(len: usize) -> Self {
        Self {
            words: vec![0; len.div_ceil(64)],
        }
    }

    pub(super) fn insert(&mut self, index: usize) {
        self.words[index / 64] |= 1 << (index % 64);
    }

    pub(super) fn remove(&mut self, index: usize) {
        self.words[index / 64] &= !(1 << (index % 64));
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        self.words[index / 64] & (1 << (index % 64)) != 0
    }

    fn union_with(&mut self, other: &Self) {
        for (word, other) in self.words.iter_mut().zip(&other.words) {
            *word |= other;
        }
    }

    fn intersect_with(&mut self, other: &Self) {
        for (word, other) in self.words.iter_mut().zip(&other.words) {
            *word &= other;
        }
    }

    fn subtract(&mut self, other: &Self) {
        for (word, other) in self.words.iter_mut().zip(&other.words) {
            *word &= !other;
        }
    }

    fn difference(&self, other: &Self) -> Self {
        let mut difference = self.clone();
        difference.subtract(other);
        difference
    }

    fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(position, word)| {
            let mut bits = *word;
            std::iter::from_fn(move || {
                (bits != 0).then(|| {
                    let bit = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    position * 64 + bit
                })
            })
        })
    }
}

/// The root local of a place.
pub(super) fn root_of(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// Whether values of `ty` need drop. The rule follows Copy: built-in
/// scalars, `None`, function names, and classes and enums marked Copy need
/// none. A generic class is never marked, so it needs drop. A generic enum
/// needs drop when one of its payloads does for these arguments. A union
/// follows its layout plan.
pub(super) fn type_needs_drop(context: &MirLoanValidationContext<'_>, ty: &Type) -> bool {
    fn visit(
        context: &MirLoanValidationContext<'_>,
        ty: &Type,
        visiting: &mut Vec<String>,
    ) -> bool {
        match ty {
            Type::Unit | Type::Function { .. } => false,
            Type::Closure { .. }
            | Type::Callable(_)
            | Type::Module(_)
            | Type::ReturnedView(_)
            | Type::TypeParam(_) => true,
            Type::Tuple(elements) => elements
                .iter()
                .any(|element| visit(context, element, visiting)),
            Type::Union(union) => {
                match context.unions.get(&crate::union_layout::union_plan_key(ty)) {
                    Some(plan) => !plan.copy,
                    None => union
                        .members
                        .iter()
                        .any(|member| visit(context, member, visiting)),
                }
            }
            Type::Named(name, args) => {
                if aura_core::types::is_builtin_copy_named_type(name, args) {
                    return false;
                }
                match (name.as_str(), args.as_slice()) {
                    ("Task" | "Lookup" | "Poll" | "QueueReceive" | "SendError", [arg]) => {
                        return visit(context, arg, visiting)
                    }
                    ("Result", [ok, error]) => {
                        return visit(context, ok, visiting) || visit(context, error, visiting)
                    }
                    ("TaskResult" | "WaitAny" | "WaitAll", [_]) | ("SelectOutcome", [_, _]) => {
                        return true
                    }
                    _ => {}
                }
                if let Some(class) = context.classes.get(name.as_str()) {
                    return !class.copy;
                }
                let Some(enum_decl) = context.enums.get(name.as_str()) else {
                    return true;
                };
                if enum_decl.copy {
                    return false;
                }
                if enum_decl.type_params.is_empty() || args.len() != enum_decl.type_params.len() {
                    return true;
                }
                // A recursive enum adds nothing on a second visit.
                let key = ty.to_string();
                if visiting.contains(&key) {
                    return false;
                }
                visiting.push(key);
                let substitutions = enum_decl
                    .type_params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect::<HashMap<_, _>>();
                let needs_drop = enum_decl
                    .variants
                    .iter()
                    .flat_map(|variant| &variant.payloads)
                    .any(|payload| {
                        visit(context, &substitute_type(payload, &substitutions), visiting)
                    });
                visiting.pop();
                needs_drop
            }
        }
    }
    visit(context, ty, &mut Vec::new())
}

/// How an instruction or a terminator touches one place.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Access {
    /// Reads the value and leaves it in place.
    Read,
    /// A consuming `Place` operand: it takes one retained reference.
    Consume,
    /// A `MovePlace` operand or a union take: it moves the value out.
    Move,
    /// A tuple take: it moves one element out and leaves the tuple.
    TakePart,
    /// Stores a new value.
    Assign,
    /// A call writes a mutable argument or receiver back.
    Writeback,
}

fn push_operand<'a>(
    accesses: &mut Vec<(&'a str, Access)>,
    operand: &'a Operand,
    role: OperandRole,
) {
    match operand {
        Operand::Place(place) => accesses.push((
            place,
            match role {
                OperandRole::Consume => Access::Consume,
                OperandRole::Borrow => Access::Read,
            },
        )),
        Operand::MovePlace(place) => accesses.push((place, Access::Move)),
        _ => {}
    }
}

fn push_rvalue_places<'a>(value: &'a Rvalue, accesses: &mut Vec<(&'a str, Access)>) {
    match value {
        Rvalue::UnionTagTest { place, .. } => accesses.push((place, Access::Read)),
        Rvalue::UnionTakePayload { place, .. } => accesses.push((place, Access::Move)),
        Rvalue::TupleTakeElement { place, .. } => accesses.push((place, Access::TakePart)),
        Rvalue::Call { callee, args, .. } => {
            let receiver_place = match callee {
                CallTarget::Member { receiver_place, .. }
                | CallTarget::TraitMember { receiver_place, .. }
                | CallTarget::Runtime { receiver_place, .. } => receiver_place.as_deref(),
                CallTarget::Name(_) | CallTarget::Value(_) | CallTarget::Extern(_) => None,
            };
            let writebacks = receiver_place
                .into_iter()
                .chain(args.iter().filter_map(|arg| arg.writeback_place.as_deref()));
            accesses.extend(writebacks.map(|place| (place, Access::Writeback)));
        }
        _ => {}
    }
}

/// Every place `instruction` touches, in the order it touches them: the
/// operands first, then the places an operation reads by name, then the
/// writebacks and the target. A loan instruction reads its source, so the
/// source stays live while the loan is.
pub(crate) fn instruction_accesses(instruction: &Instruction) -> Vec<(&str, Access)> {
    instruction_accesses_with(instruction, &[])
}

/// [`instruction_accesses`] with each operand of `borrowed` borrowed.
pub(super) fn instruction_accesses_with<'a>(
    instruction: &'a Instruction,
    borrowed: &[&Operand],
) -> Vec<(&'a str, Access)> {
    let mut accesses = Vec::new();
    for (operand, role) in operand_roles_with(instruction, borrowed) {
        push_operand(&mut accesses, operand, role);
    }
    // A moved payload or element leaves the rest of its enum or tuple.
    if let Instruction::Assign {
        value:
            Rvalue::VariantPayload {
                scrutinee: Operand::MovePlace(place),
                ..
            }
            | Rvalue::TupleElement {
                tuple: Operand::MovePlace(place),
                ..
            },
        ..
    } = instruction
    {
        accesses[0] = (place, Access::TakePart);
    }
    match instruction {
        Instruction::Safepoint
        | Instruction::Retain { .. }
        | Instruction::Release { .. }
        | Instruction::Eval { .. } => {}
        // The value moves into the temporary and back, so the place stays
        // owned throughout and only the temporary changes state.
        Instruction::Lend { place, temp, .. } => {
            accesses.push((place, Access::Read));
            accesses.push((temp, Access::Assign));
        }
        Instruction::EndLend { place, temp, .. } => {
            accesses.push((temp, Access::Move));
            accesses.push((place, Access::Read));
        }
        Instruction::IndexInRange { list, index } => {
            accesses.push((list, Access::Read));
            accesses.push((index, Access::Read));
        }
        Instruction::BeginLoan { source, .. } | Instruction::BeginElementLoan { source, .. } => {
            accesses.push((source, Access::Read))
        }
        Instruction::BeginReturnedLoan { origin, .. } => accesses.push((origin, Access::Read)),
        Instruction::Reborrow { parent, .. } | Instruction::ReborrowElement { parent, .. } => {
            accesses.push((parent, Access::Read))
        }
        Instruction::ReadLoan { target, loan } => {
            accesses.push((loan, Access::Read));
            accesses.push((target, Access::Assign));
        }
        Instruction::WriteLoan { loan, value } => {
            push_rvalue_places(value, &mut accesses);
            accesses.push((loan, Access::Read));
        }
        Instruction::EndLoan { loan } => accesses.push((loan, Access::Read)),
        Instruction::ReturnLoan { loan, origin } => {
            accesses.push((loan, Access::Read));
            accesses.push((origin, Access::Read));
        }
        Instruction::Assign { target, value } => {
            push_rvalue_places(value, &mut accesses);
            accesses.push((target, Access::Assign));
        }
        Instruction::PushCleanup { place, .. } | Instruction::PopCleanup { place, .. } => {
            accesses.push((place, Access::Read))
        }
    }
    accesses
}

/// Every place `terminator` touches.
pub(super) fn terminator_accesses(terminator: &Terminator) -> Vec<(&str, Access)> {
    let mut accesses = Vec::new();
    for (operand, role) in terminator_roles(terminator) {
        push_operand(&mut accesses, operand, role);
    }
    accesses
}

/// The loan an instruction begins and the place it borrows from.
fn begun_loan(instruction: &Instruction) -> Option<(&str, &str)> {
    match instruction {
        Instruction::BeginLoan { loan, source, .. }
        | Instruction::BeginElementLoan { loan, source, .. } => Some((loan, source)),
        Instruction::BeginReturnedLoan { loan, origin, .. } => Some((loan, origin)),
        Instruction::Reborrow { loan, parent, .. }
        | Instruction::ReborrowElement { loan, parent, .. } => Some((loan, parent)),
        _ => None,
    }
}

/// What one access means for ownership.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Touch {
    /// The place is rooted at a tracked local.
    Tracked {
        root: usize,
        /// The access names the root itself, not a part of it.
        whole: bool,
        /// A consuming `Place` operand whose value needs drop.
        retained: bool,
    },
    /// The place is rooted at a loan, which borrows the listed roots.
    Loan { retained: bool },
    /// The place's value needs no drop, or its root is unknown.
    Untracked,
}

/// What both ownership analyses know about one function's locals.
pub(super) struct OwnershipFacts<'f> {
    pub(super) function: &'f MirFunction,
    /// Root locals whose values need drop, in name order. A loan is not a
    /// root: it borrows its sources.
    pub(super) roots: Vec<&'f str>,
    index: HashMap<&'f str, usize>,
    /// The roots each loan, and each closure with borrowed captures,
    /// borrows from. A use of one is a use of them.
    pub(super) borrows: HashMap<&'f str, Vec<usize>>,
    loans: HashSet<&'f str>,
    /// Parameters and the receiver, which hold a value on entry.
    pub(super) entry: RootSet,
    /// `mut` parameters and a `mut` receiver, which the caller gets back
    /// instead of a release.
    pub(super) mutable: RootSet,
    /// Shared parameters and a shared receiver of a borrowing function
    /// (see [`borrowing_functions`]): the caller keeps the value, so the
    /// function never moves or releases it.
    pub(super) borrowed: RootSet,
    types: HashMap<&'f str, &'f Type>,
    needs_drop: HashMap<&'f str, bool>,
    /// The temporaries of the function's mutable lends.
    lent: HashSet<&'f str>,
    /// The operands the function's calls borrow: the receivers of calls on
    /// boxed generic values, and the arguments a borrowing callee keeps
    /// with the caller.
    borrowed_operands: HashSet<*const Operand>,
}

impl<'f> OwnershipFacts<'f> {
    pub(super) fn new(function: &'f MirFunction, context: &MirLoanValidationContext<'_>) -> Self {
        let mut types = HashMap::new();
        for param in &function.params {
            types.insert(param.name.as_str(), &param.ty);
        }
        for local in &function.local_types {
            types.insert(local.name.as_str(), &local.ty);
        }
        let instructions = || function.blocks.iter().flat_map(|block| &block.instructions);
        let loans = instructions()
            .filter_map(begun_loan)
            .map(|(loan, _)| loan)
            .collect::<HashSet<_>>();
        let receiver = function.receiver.map(|_| "self");
        let candidates = types
            .keys()
            .copied()
            .chain(receiver)
            .collect::<BTreeSet<_>>();
        let roots = candidates
            .into_iter()
            .filter(|name| !loans.contains(name))
            .filter(|name| {
                types
                    .get(name)
                    .is_none_or(|ty| type_needs_drop(context, ty))
            })
            .collect::<Vec<_>>();
        let index = roots
            .iter()
            .enumerate()
            .map(|(index, root)| (*root, index))
            .collect::<HashMap<_, _>>();
        let mut entry = RootSet::new(roots.len());
        let mut mutable = RootSet::new(roots.len());
        let mut borrowed = RootSet::new(roots.len());
        let kept = context.borrowing_function(&function.name);
        let mut holder = |name: &str, passing: MirReceiverKind, kept: Option<MirReceiverKind>| {
            if let Some(&root) = index.get(name) {
                entry.insert(root);
                match passing {
                    MirReceiverKind::BorrowMut => mutable.insert(root),
                    MirReceiverKind::Borrow if kept == Some(MirReceiverKind::Borrow) => {
                        borrowed.insert(root)
                    }
                    _ => {}
                }
            }
        };
        if let Some(passing) = function.receiver {
            holder("self", passing, kept.and_then(|callee| callee.receiver));
        }
        for (position, param) in function.params.iter().enumerate() {
            let kind = kept.and_then(|callee| callee.params.get(position).copied());
            holder(&param.name, param.passing, kind);
        }
        let mut facts = Self {
            function,
            roots,
            index,
            borrows: HashMap::new(),
            loans,
            entry,
            mutable,
            borrowed,
            types,
            needs_drop: HashMap::new(),
            borrowed_operands: instructions()
                .flat_map(|instruction| {
                    borrowed_operands(instruction, context, |place| {
                        context.place_type(function, place).ok().flatten()
                    })
                })
                .map(|operand| operand as *const Operand)
                .collect(),
            lent: instructions()
                .filter_map(|instruction| match instruction {
                    Instruction::Lend {
                        temp,
                        mutable: true,
                        ..
                    } => Some(temp.as_str()),
                    _ => None,
                })
                .collect(),
        };
        facts.borrows = facts.borrowed_roots();
        facts
    }

    /// The operands of `instruction` that its call borrows.
    pub(super) fn borrowed_in(&self, instruction: &'f Instruction) -> Vec<&'f Operand> {
        if self.borrowed_operands.is_empty() {
            return Vec::new();
        }
        operand_roles(instruction)
            .into_iter()
            .map(|(operand, _)| operand)
            .filter(|operand| {
                self.borrowed_operands
                    .contains(&(*operand as *const Operand))
            })
            .collect()
    }

    /// The roots the caller keeps: `mut` parameters, which come back, and
    /// the borrowed shared parameters of a borrowing function.
    pub(super) fn kept(&self) -> RootSet {
        let mut kept = self.mutable.clone();
        kept.union_with(&self.borrowed);
        kept
    }

    /// What `instruction` does to each place it touches. The call of a
    /// mutable lend takes the temporary's value and returns it through the
    /// writeback, so the temporary moves in instead of taking a retained
    /// reference: see `mir_lending.rs`.
    pub(super) fn accesses(&self, instruction: &'f Instruction) -> Vec<(&'f str, Access)> {
        let mut accesses = instruction_accesses_with(instruction, &self.borrowed_in(instruction));
        if self.lent.is_empty() {
            return accesses;
        }
        let written_back = accesses
            .iter()
            .filter(|(place, access)| *access == Access::Writeback && self.lent.contains(place))
            .map(|(place, _)| *place)
            .collect::<Vec<_>>();
        for (place, access) in &mut accesses {
            if *access == Access::Consume && written_back.contains(place) {
                *access = Access::Move;
            }
        }
        accesses
    }

    /// The roots each loan and each borrowing closure borrows from. A loan
    /// of a loan borrows what its parent borrows, and a closure copied to
    /// another local keeps its borrows there.
    fn borrowed_roots(&self) -> HashMap<&'f str, Vec<usize>> {
        // `(borrower, source, direct)`: a direct borrower borrows the
        // source root itself; a copy only carries the source's borrows.
        let mut edges = Vec::<(&'f str, &'f str, bool)>::new();
        for instruction in self
            .function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
        {
            if let Some((loan, source)) = begun_loan(instruction) {
                edges.push((loan, root_of(source), true));
                continue;
            }
            let Instruction::Assign { target, value } = instruction else {
                continue;
            };
            match value {
                Rvalue::Closure { captures, .. } => {
                    for capture in captures
                        .iter()
                        .filter(|capture| capture.passing != MirReceiverKind::Value)
                    {
                        let source = match (&capture.source_place, &capture.value) {
                            (Some(source), _) => Some(source.as_str()),
                            (None, Operand::Place(place) | Operand::MovePlace(place)) => {
                                Some(place.as_str())
                            }
                            (None, _) => None,
                        };
                        edges.extend(source.map(|source| (target.as_str(), root_of(source), true)));
                    }
                }
                // The validator proves a payload's variant through the place
                // it was read from, so the payload keeps that place live.
                Rvalue::VariantPayload {
                    scrutinee: Operand::Place(source) | Operand::MovePlace(source),
                    ..
                } => edges.push((target.as_str(), root_of(source), true)),
                Rvalue::Use(Operand::Place(source) | Operand::MovePlace(source))
                | Rvalue::CallableAdapt {
                    value: Operand::Place(source) | Operand::MovePlace(source),
                    ..
                } => edges.push((target.as_str(), root_of(source), false)),
                _ => {}
            }
        }
        let mut borrows = HashMap::<&'f str, Vec<usize>>::new();
        for loan in &self.loans {
            borrows.entry(loan).or_default();
        }
        // Each round carries borrows one step further, so the number of
        // edges bounds the rounds.
        for _ in 0..=edges.len() {
            let mut changed = false;
            for &(borrower, source, direct) in &edges {
                let mut added = match borrows.get(source) {
                    Some(sources) => sources.clone(),
                    None => Vec::new(),
                };
                if direct {
                    added.extend(self.index.get(source).copied());
                }
                if added.is_empty() {
                    continue;
                }
                let entry = borrows.entry(borrower).or_default();
                for root in added {
                    if !entry.contains(&root) {
                        entry.push(root);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for sources in borrows.values_mut() {
            sources.sort_unstable();
        }
        borrows
    }

    pub(super) fn tracked(&self, root: &str) -> Option<usize> {
        self.index.get(root).copied()
    }

    /// Whether the value at `place` needs drop. A place whose type the
    /// module does not spell needs drop.
    pub(super) fn place_needs_drop(
        &mut self,
        context: &MirLoanValidationContext<'_>,
        place: &'f str,
    ) -> bool {
        if let Some(needs_drop) = self.needs_drop.get(place) {
            return *needs_drop;
        }
        let root = root_of(place);
        let needs_drop = if root.len() == place.len() {
            self.types
                .get(root)
                .is_none_or(|ty| type_needs_drop(context, ty))
        } else {
            match context.place_type(self.function, place) {
                Ok(Some(ty)) => type_needs_drop(context, &ty),
                _ => true,
            }
        };
        self.needs_drop.insert(place, needs_drop);
        needs_drop
    }

    /// What `access` to `place` means for ownership.
    pub(super) fn touch(
        &mut self,
        context: &MirLoanValidationContext<'_>,
        place: &'f str,
        access: Access,
    ) -> Touch {
        let root = root_of(place);
        let consumed = access == Access::Consume;
        if self.loans.contains(root) {
            return Touch::Loan {
                retained: consumed && self.place_needs_drop(context, place),
            };
        }
        match self.tracked(root) {
            Some(index) => {
                let whole = root.len() == place.len();
                Touch::Tracked {
                    root: index,
                    whole,
                    retained: consumed && (whole || self.place_needs_drop(context, place)),
                }
            }
            None => Touch::Untracked,
        }
    }

    pub(super) fn borrowed_by(&self, name: &str) -> &[usize] {
        self.borrows.get(name).map_or(&[], Vec::as_slice)
    }

    fn root_type(&self, root: usize) -> Option<Type> {
        self.types.get(self.roots[root]).map(|ty| (*ty).clone())
    }
}

/// The reachable blocks of a function and the edges between them.
pub(super) struct FlowGraph {
    pub(super) reachable: Vec<bool>,
    pub(super) successors: Vec<Vec<usize>>,
    pub(super) predecessors: Vec<Vec<usize>>,
    pub(super) entry: usize,
    /// The blocks whose fall-through edge the graph leaves out.
    pub(super) closed: Vec<usize>,
}

impl FlowGraph {
    /// The graph of `function`, or `None` when a label repeats, the entry
    /// is missing, or a terminator names an unknown block.
    ///
    /// The graph leaves out the fall-through edge of a match chain whose
    /// arms cover every variant of the scrutinee's enum. Lowering emits that
    /// edge, but no value can take it, and the paths through it would
    /// otherwise join moved values with live ones.
    pub(super) fn new(
        function: &MirFunction,
        context: &MirLoanValidationContext<'_>,
    ) -> Option<Self> {
        let mut labels = HashMap::new();
        for (index, block) in function.blocks.iter().enumerate() {
            if labels.insert(block.label.as_str(), index).is_some() {
                return None;
            }
        }
        let entry = *labels.get(function.entry.as_str())?;
        let mut successors = Vec::with_capacity(function.blocks.len());
        for block in &function.blocks {
            let mut targets = Vec::new();
            for label in mir_successors(&block.terminator) {
                let target = *labels.get(label)?;
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
            successors.push(targets);
        }
        let mut closed = Vec::new();
        for (block, otherwise) in exhaustive_matches(function, context, &labels, &successors) {
            successors[block].retain(|target| *target != otherwise);
            closed.push(block);
        }
        let mut reachable = vec![false; function.blocks.len()];
        let mut pending = vec![entry];
        while let Some(block) = pending.pop() {
            if !std::mem::replace(&mut reachable[block], true) {
                pending.extend(successors[block].iter().copied());
            }
        }
        let mut predecessors = vec![Vec::new(); function.blocks.len()];
        for (block, targets) in successors.iter().enumerate() {
            if reachable[block] {
                for target in targets {
                    predecessors[*target].push(block);
                }
            }
        }
        Some(Self {
            reachable,
            successors,
            predecessors,
            entry,
            closed,
        })
    }

    pub(super) fn reachable_count(&self) -> usize {
        self.reachable
            .iter()
            .filter(|reachable| **reachable)
            .count()
    }

    /// The edges that reach `block`, counting a function's start as one
    /// into its entry.
    fn incoming(&self, block: usize) -> usize {
        self.predecessors[block].len() + usize::from(block == self.entry)
    }
}

type MatchParts<'b> = (&'b str, &'b [super::MirMatchArm], &'b str);

fn scrutinee_of(block: &BasicBlock) -> Option<MatchParts<'_>> {
    match &block.terminator {
        Terminator::Match {
            scrutinee: Operand::Place(place) | Operand::MovePlace(place),
            arms,
            otherwise,
        } => Some((place.as_str(), arms, otherwise.as_str())),
        _ => None,
    }
}

/// Whether `block` holds nothing but releases, which the pass may have put
/// at the start of a link in a match chain.
fn only_releases(block: &BasicBlock) -> bool {
    block
        .instructions
        .iter()
        .all(|instruction| matches!(instruction, Instruction::Release { .. }))
}

/// The `(block, fall-through block)` pairs of the match chains that cover
/// every variant of their scrutinee's enum. A chain continues through a
/// fall-through block that holds only releases, has no other way in, and
/// matches the same place.
fn exhaustive_matches(
    function: &MirFunction,
    context: &MirLoanValidationContext<'_>,
    labels: &HashMap<&str, usize>,
    successors: &[Vec<usize>],
) -> Vec<(usize, usize)> {
    let mut incoming = vec![0usize; function.blocks.len()];
    for targets in successors {
        for target in targets {
            incoming[*target] += 1;
        }
    }
    let mut exhaustive = Vec::new();
    for (index, block) in function.blocks.iter().enumerate() {
        let Some((place, _, _)) = scrutinee_of(block) else {
            continue;
        };
        // Start at the head of each chain.
        let continues = |candidate: &BasicBlock| {
            scrutinee_of(candidate)
                .is_some_and(|(other, _, otherwise)| other == place && otherwise == block.label)
        };
        if only_releases(block) && incoming[index] == 1 && function.blocks.iter().any(continues) {
            continue;
        }
        let Ok(Some(Type::Named(name, _))) = context.place_type(function, place) else {
            continue;
        };
        let Some(enumeration) = context.enums.get(name.as_str()) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        let mut current = index;
        for _ in 0..function.blocks.len() {
            let Some((_, arms, otherwise)) = scrutinee_of(&function.blocks[current]) else {
                break;
            };
            for arm in arms {
                match &arm.variant_name {
                    _ if arm.wildcard => seen.extend(
                        enumeration
                            .variants
                            .iter()
                            .map(|variant| variant.name.as_str()),
                    ),
                    Some(variant) => {
                        seen.insert(variant.as_str());
                    }
                    None => {}
                }
            }
            let Some(&next) = labels.get(otherwise) else {
                break;
            };
            if enumeration
                .variants
                .iter()
                .all(|variant| seen.contains(variant.name.as_str()))
            {
                if arms.iter().all(|arm| arm.label != otherwise) {
                    exhaustive.push((current, next));
                }
                break;
            }
            let chained = &function.blocks[next];
            if !only_releases(chained)
                || incoming[next] != 1
                || scrutinee_of(chained).is_none_or(|(other, _, _)| other != place)
            {
                break;
            }
            current = next;
        }
    }
    exhaustive
}

/// The ForRange binding a terminator defines on the edge to `successor`.
pub(super) fn edge_binding(function: &MirFunction, from: usize, successor: usize) -> Option<&str> {
    match &function.blocks[from].terminator {
        Terminator::ForRange {
            binding,
            body_label,
            ..
        } if function.blocks[successor].label == *body_label => Some(binding),
        _ => None,
    }
}

/// What one instruction or terminator does to the tracked roots, for the
/// pass.
#[derive(Default)]
struct Effect<'f> {
    /// Roots that must hold a value: every touch but a whole store.
    uses: Vec<usize>,
    /// Roots the operation empties.
    moves: Vec<usize>,
    /// Roots the operation fills: a whole store or writeback.
    defs: Vec<usize>,
    /// Whole stores, which need the root empty first.
    assigns: Vec<usize>,
    /// Places to retain, one per consuming operand.
    retains: Vec<&'f str>,
}

impl<'f> Effect<'f> {
    fn of(
        facts: &mut OwnershipFacts<'f>,
        context: &MirLoanValidationContext<'_>,
        accesses: Vec<(&'f str, Access)>,
    ) -> Self {
        let mut effect = Self::default();
        for (place, access) in accesses {
            let borrowed = facts.borrowed_by(root_of(place)).to_vec();
            effect.uses.extend(borrowed);
            match facts.touch(context, place, access) {
                Touch::Loan { retained } => {
                    if retained {
                        effect.retains.push(place);
                    }
                }
                Touch::Tracked {
                    root,
                    whole,
                    retained,
                } => {
                    match (access, whole) {
                        (Access::Assign, true) => {
                            effect.defs.push(root);
                            effect.assigns.push(root);
                        }
                        (Access::Writeback, true) => effect.defs.push(root),
                        (Access::Move, true) => {
                            effect.uses.push(root);
                            effect.moves.push(root);
                        }
                        _ => effect.uses.push(root),
                    }
                    if retained {
                        effect.retains.push(place);
                    }
                }
                Touch::Untracked => {}
            }
        }
        effect
    }

    fn live_before(&self, live_after: &RootSet) -> RootSet {
        let mut live = live_after.clone();
        for root in &self.defs {
            live.remove(*root);
        }
        for root in &self.uses {
            live.insert(*root);
        }
        live
    }
}

/// A reachable block's effects and the roots live after each instruction.
struct BlockFlow<'f> {
    instructions: Vec<Effect<'f>>,
    terminator: Effect<'f>,
    /// The roots live after each instruction.
    live_after: Vec<RootSet>,
    live_in: RootSet,
}

/// The rewritten blocks and new locals of one function.
struct FunctionOwnership {
    blocks: Vec<BasicBlock>,
    locals: Vec<MirLocalType>,
}

/// States ownership in an implicit module: see the module comment. A module
/// that already states it is left alone, so the pass is idempotent. A
/// module the pass cannot read, or one with a function too large for the
/// analysis, stays implicit, and the validator reports what is wrong with
/// it.
pub(crate) fn insert_ownership(module: &mut MirModule) {
    insert_ownership_within(module, MAX_OWNERSHIP_STATE_CELLS);
}

/// [`insert_ownership`] with at most `state_cells` tracked roots times
/// reachable blocks per function.
fn insert_ownership_within(module: &mut MirModule, state_cells: usize) {
    if module.ownership == MirOwnership::Explicit {
        return;
    }
    module.borrowing = borrowing_functions(module);
    let rewrites = (|| {
        let context = MirLoanValidationContext::new(module).ok()?;
        let mut rewrites = Vec::new();
        for (index, function) in module
            .functions
            .iter()
            .chain(module.top_level.iter())
            .enumerate()
        {
            // Lends come first, so the retains and releases cover their
            // temporaries too.
            let lent = super::lending::lend_function(function, &context);
            let subject = lent.as_ref().unwrap_or(function);
            match function_ownership(subject, &context, state_cells) {
                Ok(Some(mut rewrite)) => {
                    if lent.is_some() {
                        rewrite.locals.extend(lent_locals(function, subject));
                    }
                    rewrites.push((index, rewrite));
                }
                Ok(None) => {
                    if let Some(lent) = &lent {
                        rewrites.push((
                            index,
                            FunctionOwnership {
                                blocks: lent.blocks.clone(),
                                locals: lent_locals(function, lent),
                            },
                        ));
                    }
                }
                Err(()) => return None,
            }
        }
        Some(rewrites)
    })();
    let Some(rewrites) = rewrites else {
        // The module stays implicit, and an implicit module keeps nothing.
        module.borrowing.clear();
        return;
    };
    let function_count = module.functions.len();
    for (index, rewrite) in rewrites {
        let function = match module.functions.get_mut(index) {
            Some(function) => function,
            None => {
                debug_assert_eq!(index, function_count);
                module
                    .top_level
                    .as_mut()
                    .expect("a rewrite past the functions is the top level")
            }
        };
        function.blocks = rewrite.blocks;
        if !rewrite.locals.is_empty() {
            function.local_types.extend(rewrite.locals);
            function
                .local_types
                .sort_by(|left, right| left.name.cmp(&right.name));
        }
    }
    module.ownership = MirOwnership::Explicit;
    cancel_retain_release_pairs(module);
}

/// The temporaries `lent` declares that `function` does not.
fn lent_locals(function: &MirFunction, lent: &MirFunction) -> Vec<MirLocalType> {
    lent.local_types
        .iter()
        .filter(|local| {
            !function
                .local_types
                .iter()
                .any(|existing| existing.name == local.name)
        })
        .cloned()
        .collect()
}

/// One retain the cancellation turns into a move: its block, the retain's
/// position, the consumer's position (`None` for the terminator), the
/// release's position (`None` when the consumer is a `Return`), and the
/// root.
struct MovePair {
    block: usize,
    retain: usize,
    consumer: Option<usize>,
    release: Option<usize>,
    root: String,
}

/// Turns `Retain(x); <consume Place(x)>; ...; Release(x)` into
/// `<consume MovePlace(x)>` when nothing between the consumer and the
/// release uses `x`. A `Return(Place(x))` releases its root, so a retain
/// right before it becomes a move too. Only whole roots move: a retained
/// projection leaves its root holding the rest of the value.
///
/// A move needs owned authority over the root, which the loan validator
/// decides. Each rewritten function must pass the loan validator and the
/// ownership checker. A function that fails with every pair keeps only the
/// pairs that pass one at a time.
pub(crate) fn cancel_retain_release_pairs(module: &mut MirModule) {
    if module.ownership != MirOwnership::Explicit {
        return;
    }
    let rewrites = {
        let Ok(context) = MirLoanValidationContext::new(module) else {
            return;
        };
        let repeatable = repeatable_captures(module);
        let none = HashSet::new();
        module
            .functions
            .iter()
            .chain(module.top_level.iter())
            .enumerate()
            .filter_map(|(index, function)| {
                let pinned = repeatable.get(function.name.as_str()).unwrap_or(&none);
                cancelled_function(function, &context, pinned).map(|function| (index, function))
            })
            .collect::<Vec<_>>()
    };
    let function_count = module.functions.len();
    for (index, rewrite) in rewrites {
        let function = match module.functions.get_mut(index) {
            Some(function) => function,
            None => {
                debug_assert_eq!(index, function_count);
                module
                    .top_level
                    .as_mut()
                    .expect("a rewrite past the functions is the top level")
            }
        };
        *function = rewrite;
    }
}

/// The captures of each closure body that a repeatable closure passes, by
/// body name. Each call reads them again, so the body never moves them.
fn repeatable_captures(module: &MirModule) -> HashMap<&str, HashSet<&str>> {
    let mut captures = HashMap::<&str, HashSet<&str>>::new();
    for instruction in module
        .functions
        .iter()
        .chain(module.top_level.iter())
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
    {
        if let Instruction::Assign {
            value:
                Rvalue::Closure {
                    function,
                    captures: closure_captures,
                    consuming: false,
                    ..
                },
            ..
        } = instruction
        {
            captures
                .entry(function.as_str())
                .or_default()
                .extend(closure_captures.iter().map(|capture| capture.name.as_str()));
        }
    }
    captures
}

/// `function` with its pairs cancelled, or `None` when no pair cancels.
/// A `pinned` root never moves.
fn cancelled_function(
    function: &MirFunction,
    context: &MirLoanValidationContext<'_>,
    pinned: &HashSet<&str>,
) -> Option<MirFunction> {
    let mut pairs = move_pairs(function, context);
    pairs.retain(|pair| !pinned.contains(pair.root.as_str()));
    if pairs.is_empty() {
        return None;
    }
    let valid = |candidate: &MirFunction| {
        super::validate_function_loan_flow(candidate, context).is_ok()
            && super::ownership_check::check_function(
                candidate,
                context,
                MAX_OWNERSHIP_STATE_CELLS,
                None,
            )
            .is_ok()
    };
    let mut candidate = function.clone();
    apply_move_pairs(&mut candidate, &pairs);
    if valid(&candidate) {
        return Some(candidate);
    }
    let mut kept = Vec::new();
    for pair in pairs {
        kept.push(pair);
        let mut candidate = function.clone();
        apply_move_pairs(&mut candidate, &kept);
        if !valid(&candidate) {
            kept.pop();
        }
    }
    if kept.is_empty() {
        return None;
    }
    let mut candidate = function.clone();
    apply_move_pairs(&mut candidate, &kept);
    Some(candidate)
}

/// The retains of `function` that cancel against a release.
fn move_pairs(function: &MirFunction, context: &MirLoanValidationContext<'_>) -> Vec<MovePair> {
    let mut facts = OwnershipFacts::new(function, context);
    let mut pairs = Vec::new();
    if facts.roots.is_empty() {
        return pairs;
    }
    // A value read through a loan, or one that borrows its source, has a
    // borrowed origin, and the loan validator refuses to move it.
    let read_from_loans = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::ReadLoan { target, .. } => Some(target.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    for (block_index, block) in function.blocks.iter().enumerate() {
        let instructions = &block.instructions;
        for (retain, instruction) in instructions.iter().enumerate() {
            let Instruction::Retain { place } = instruction else {
                continue;
            };
            let Some(root) = facts.tracked(place) else {
                continue;
            };
            if !context.root_allows_owned_take(function, place)
                || read_from_loans.contains(place.as_str())
                || !facts.borrowed_by(place).is_empty()
            {
                continue;
            }
            // The retains of one consumer come right before it.
            let consumer = (retain + 1..instructions.len())
                .find(|position| !matches!(instructions[*position], Instruction::Retain { .. }));
            // An element loan selects with a place it keeps.
            if consumer.is_some_and(|position| {
                matches!(
                    instructions[position],
                    Instruction::BeginElementLoan { .. } | Instruction::ReborrowElement { .. }
                )
            }) {
                continue;
            }
            let accesses = match consumer {
                Some(position) => facts.accesses(&instructions[position]),
                None => terminator_accesses(&block.terminator),
            };
            if !sole_consume(&facts, &accesses, place, root) {
                continue;
            }
            let release = match consumer {
                None if matches!(block.terminator, Terminator::Return(_)) => None,
                None => continue,
                Some(position) => {
                    match later_release(&mut facts, instructions, position + 1, place, root) {
                        Some(release) => Some(release),
                        None => continue,
                    }
                }
            };
            pairs.push(MovePair {
                block: block_index,
                retain,
                consumer,
                release,
                root: place.clone(),
            });
        }
    }
    pairs
}

/// Whether the only touch of `root` among `accesses` is one consuming
/// `Place(name)` of the whole root. A loan or a closure that borrows the
/// root touches it too.
fn sole_consume(
    facts: &OwnershipFacts<'_>,
    accesses: &[(&str, Access)],
    name: &str,
    root: usize,
) -> bool {
    let mut consumes = 0;
    for (place, access) in accesses {
        let touches = root_of(place) == name || facts.borrowed_by(root_of(place)).contains(&root);
        if !touches {
            continue;
        }
        if *place != name || *access != Access::Consume {
            return false;
        }
        consumes += 1;
    }
    consumes == 1
}

/// The position of the first `Release(name)` from `start` on, when nothing
/// before it uses the root: no access, no retain, and no `try` that would
/// release it on its error edge.
fn later_release(
    facts: &mut OwnershipFacts<'_>,
    instructions: &[Instruction],
    start: usize,
    name: &str,
    root: usize,
) -> Option<usize> {
    for (position, instruction) in instructions.iter().enumerate().skip(start) {
        match instruction {
            Instruction::Release { place } if place == name => return Some(position),
            Instruction::Release { .. } => continue,
            Instruction::Retain { place } if root_of(place) == name => return None,
            _ => {}
        }
        if try_releases(instruction).is_some_and(|listed| listed.iter().any(|item| item == name)) {
            return None;
        }
        let touches = instruction_accesses(instruction).iter().any(|(place, _)| {
            root_of(place) == name || facts.borrowed_by(root_of(place)).contains(&root)
        });
        if touches {
            return None;
        }
    }
    None
}

/// The error-edge releases of a `try` instruction.
fn try_releases(instruction: &Instruction) -> Option<&Vec<String>> {
    match instruction {
        Instruction::Assign {
            value: Rvalue::Try {
                releases_on_error, ..
            },
            ..
        }
        | Instruction::WriteLoan {
            value: Rvalue::Try {
                releases_on_error, ..
            },
            ..
        } => Some(releases_on_error),
        _ => None,
    }
}

fn apply_move_pairs(function: &mut MirFunction, pairs: &[MovePair]) {
    let mut removed = vec![HashSet::<usize>::new(); function.blocks.len()];
    for pair in pairs {
        let block = &mut function.blocks[pair.block];
        let operand = match pair.consumer {
            Some(position) => {
                let consumer = &mut block.instructions[position];
                // The root moves into a `try`, so its error edge no longer
                // releases it.
                if let Some(listed) = try_releases(consumer) {
                    let kept = listed
                        .iter()
                        .filter(|item| **item != pair.root)
                        .cloned()
                        .collect::<Vec<_>>();
                    set_try_releases(consumer, || kept);
                }
                place_operand_mut(consumer, &pair.root)
            }
            None => match &mut block.terminator {
                Terminator::Return(operand) => Some(operand),
                _ => None,
            },
        };
        let Some(operand) = operand else {
            debug_assert!(false, "a cancelled retain names its consumer's operand");
            continue;
        };
        *operand = Operand::MovePlace(pair.root.clone());
        removed[pair.block].insert(pair.retain);
        removed[pair.block].extend(pair.release);
    }
    for (block, removed) in function.blocks.iter_mut().zip(removed) {
        if removed.is_empty() {
            continue;
        }
        let mut position = 0;
        block.instructions.retain(|_| {
            position += 1;
            !removed.contains(&(position - 1))
        });
    }
}

/// The `Place(name)` operand of `instruction`.
fn place_operand_mut<'a>(instruction: &'a mut Instruction, name: &str) -> Option<&'a mut Operand> {
    let mut operands = Vec::new();
    match instruction {
        Instruction::BeginElementLoan { selector, .. }
        | Instruction::ReborrowElement { selector, .. } => operands.push(selector),
        Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
            rvalue_operands_mut(value, &mut operands)
        }
        Instruction::Eval { value } => operands.push(value),
        Instruction::Safepoint
        | Instruction::Retain { .. }
        | Instruction::Release { .. }
        | Instruction::Lend { .. }
        | Instruction::EndLend { .. }
        | Instruction::IndexInRange { .. }
        | Instruction::BeginLoan { .. }
        | Instruction::BeginReturnedLoan { .. }
        | Instruction::Reborrow { .. }
        | Instruction::ReadLoan { .. }
        | Instruction::EndLoan { .. }
        | Instruction::ReturnLoan { .. }
        | Instruction::PushCleanup { .. }
        | Instruction::PopCleanup { .. } => {}
    }
    operands
        .into_iter()
        .find(|operand| matches!(operand, Operand::Place(place) if place == name))
}

/// Every operand of `value`, as `rvalue_roles` lists them.
fn rvalue_operands_mut<'a>(value: &'a mut Rvalue, operands: &mut Vec<&'a mut Operand>) {
    match value {
        Rvalue::Use(operand)
        | Rvalue::CallableAdapt { value: operand, .. }
        | Rvalue::UnionInject { value: operand, .. }
        | Rvalue::Try { value: operand, .. }
        | Rvalue::NoneTest { value: operand }
        | Rvalue::Unary { value: operand, .. }
        | Rvalue::Cast { value: operand, .. }
        | Rvalue::TupleElement { tuple: operand, .. }
        | Rvalue::VariantPayload {
            scrutinee: operand, ..
        }
        | Rvalue::Member {
            object: operand, ..
        } => operands.push(operand),
        Rvalue::UnionTagTest { .. }
        | Rvalue::UnionTakePayload { .. }
        | Rvalue::TupleTakeElement { .. }
        | Rvalue::ModuleConstant { .. } => {}
        Rvalue::Closure { captures, .. } => {
            operands.extend(captures.iter_mut().map(|capture| &mut capture.value))
        }
        Rvalue::FormatString { parts } => {
            for part in parts {
                match part {
                    MirFormatPart::Literal(_) => {}
                    MirFormatPart::Value(operand)
                    | MirFormatPart::Int64(operand)
                    | MirFormatPart::Formatted { value: operand, .. } => operands.push(operand),
                }
            }
        }
        Rvalue::Binary { left, right, .. } => {
            operands.push(left);
            operands.push(right);
        }
        Rvalue::StartTask {
            stack_size,
            task_group,
            function,
            args,
            ..
        } => {
            operands.extend(stack_size.iter_mut());
            operands.push(task_group);
            operands.push(function);
            operands.extend(args.iter_mut().map(|arg| &mut arg.value));
        }
        Rvalue::Call { callee, args, .. } => {
            match callee {
                CallTarget::Name(_) | CallTarget::Extern(_) => {}
                CallTarget::Value(operand)
                | CallTarget::Member {
                    object: operand, ..
                }
                | CallTarget::TraitMember {
                    object: operand, ..
                } => operands.push(operand),
                CallTarget::Runtime { receiver, .. } => operands.extend(receiver.iter_mut()),
            }
            operands.extend(args.iter_mut().map(|arg| &mut arg.value));
        }
        Rvalue::VecLiteral { elements, .. }
        | Rvalue::TupleLiteral { elements, .. }
        | Rvalue::SetLiteral { elements, .. } => operands.extend(elements.iter_mut()),
        Rvalue::MapLiteral { entries, .. } => {
            for entry in entries {
                operands.push(&mut entry.key);
                operands.push(&mut entry.value);
            }
        }
        Rvalue::Construct { fields, .. } => {
            operands.extend(fields.iter_mut().map(|field| &mut field.value))
        }
        Rvalue::EnumVariant { payloads, .. } => operands.extend(payloads.iter_mut()),
    }
}

/// Plans one function. `Ok(None)` leaves it as it is, because nothing in it
/// needs drop. `Err` means the function cannot be analysed.
fn function_ownership(
    function: &MirFunction,
    context: &MirLoanValidationContext<'_>,
    state_cells: usize,
) -> Result<Option<FunctionOwnership>, ()> {
    let mut facts = OwnershipFacts::new(function, context);
    if facts.roots.is_empty() {
        return Ok(None);
    }
    let graph = FlowGraph::new(function, context).ok_or(())?;
    if facts.roots.len().saturating_mul(graph.reachable_count()) > state_cells {
        return Err(());
    }
    let width = facts.roots.len();
    let mut flows = Vec::with_capacity(function.blocks.len());
    for (index, block) in function.blocks.iter().enumerate() {
        if !graph.reachable[index] {
            flows.push(None);
            continue;
        }
        let instructions = block
            .instructions
            .iter()
            .map(|instruction| {
                let accesses = facts.accesses(instruction);
                Effect::of(&mut facts, context, accesses)
            })
            .collect::<Vec<_>>();
        let mut terminator =
            Effect::of(&mut facts, context, terminator_accesses(&block.terminator));
        // A return hands `mut` parameters back to the caller.
        if matches!(block.terminator, Terminator::Return(_)) {
            terminator.uses.extend(facts.kept().iter());
        }
        flows.push(Some(BlockFlow {
            instructions,
            terminator,
            live_after: Vec::new(),
            live_in: RootSet::new(width),
        }));
    }
    // A block that ends the program releases nothing: the abort reclaims
    // every value, and an initializer may not mention `self` there.
    let doomed = function
        .blocks
        .iter()
        .map(|block| {
            matches!(
                block.terminator,
                Terminator::Unreachable | Terminator::AssertFail { .. }
            )
        })
        .collect::<Vec<_>>();
    let edge_def = |from: usize, successor: usize| {
        edge_binding(function, from, successor).and_then(|binding| facts.tracked(binding))
    };

    // Backward liveness to a fixed point.
    let mut pending = (0..function.blocks.len())
        .filter(|block| graph.reachable[*block])
        .collect::<Vec<_>>();
    let mut queued = graph.reachable.clone();
    while let Some(block) = pending.pop() {
        queued[block] = false;
        let mut live = RootSet::new(width);
        for &successor in &graph.successors[block] {
            let mut incoming = flows[successor]
                .as_ref()
                .expect("a successor of a reachable block is reachable")
                .live_in
                .clone();
            if let Some(binding) = edge_def(block, successor) {
                incoming.remove(binding);
            }
            live.union_with(&incoming);
        }
        let flow = flows[block].as_mut().expect("pending blocks are reachable");
        live = flow.terminator.live_before(&live);
        for effect in flow.instructions.iter().rev() {
            live = effect.live_before(&live);
        }
        if live != flow.live_in {
            flow.live_in = live;
            for &predecessor in &graph.predecessors[block] {
                if !std::mem::replace(&mut queued[predecessor], true) {
                    pending.push(predecessor);
                }
            }
        }
    }
    // Each instruction's live-after set comes from the successors' live-in
    // sets, which a copy keeps readable while the flows change.
    let live_ins = flows
        .iter()
        .map(|flow| flow.as_ref().map(|flow| flow.live_in.clone()))
        .collect::<Vec<_>>();
    for (block, flow) in flows.iter_mut().enumerate() {
        let Some(flow) = flow.as_mut() else {
            continue;
        };
        let mut live = RootSet::new(width);
        for &successor in &graph.successors[block] {
            let mut incoming = live_ins[successor].clone().expect("reachable successor");
            if let Some(binding) = edge_def(block, successor) {
                incoming.remove(binding);
            }
            live.union_with(&incoming);
        }
        live = flow.terminator.live_before(&live);
        flow.live_after = vec![RootSet::new(width); flow.instructions.len()];
        for index in (0..flow.instructions.len()).rev() {
            flow.live_after[index] = live.clone();
            live = flow.instructions[index].live_before(&live);
        }
    }

    // Forward: which roots hold a value on some path at each block's
    // start, after its start releases.
    let mut starts: Vec<Option<RootSet>> = vec![None; function.blocks.len()];
    let mut entry = facts.entry.clone();
    entry.intersect_with(
        live_ins[graph.entry]
            .as_ref()
            .expect("the entry is reachable"),
    );
    starts[graph.entry] = Some(entry);
    let mut pending = vec![graph.entry];
    let mut queued = vec![false; function.blocks.len()];
    queued[graph.entry] = true;
    while let Some(block) = pending.pop() {
        queued[block] = false;
        let flow = flows[block].as_ref().expect("pending blocks are reachable");
        let mut may = starts[block].clone().expect("pending blocks have a start");
        for (effect, live_after) in flow.instructions.iter().zip(&flow.live_after) {
            step(effect, live_after, &mut may);
        }
        for root in &flow.terminator.moves {
            may.remove(*root);
        }
        for &successor in &graph.successors[block] {
            let mut outgoing = may.clone();
            if let Some(binding) = edge_def(block, successor) {
                outgoing.insert(binding);
            }
            outgoing.intersect_with(live_ins[successor].as_ref().expect("reachable successor"));
            let changed = match &mut starts[successor] {
                Some(start) => {
                    let before = start.clone();
                    start.union_with(&outgoing);
                    *start != before
                }
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

    // Releases on edges: at the successor's start when the successor has
    // one way in or the edge leaves a `for` header, whose exit the
    // interpreter matches by label; otherwise in a new block on the edge.
    let mut start_releases = vec![RootSet::new(width); function.blocks.len()];
    {
        let mut unused = facts.entry.clone();
        unused.subtract(
            live_ins[graph.entry]
                .as_ref()
                .expect("the entry is reachable"),
        );
        start_releases[graph.entry].union_with(&unused);
    }
    let mut labels = function
        .blocks
        .iter()
        .map(|block| block.label.clone())
        .collect::<HashSet<_>>();
    let mut edge_blocks = Vec::<BasicBlock>::new();
    let mut retargets = vec![Vec::<(usize, String)>::new(); function.blocks.len()];
    let mut exits = vec![None; function.blocks.len()];
    for block in 0..function.blocks.len() {
        let Some(flow) = flows[block].as_ref() else {
            continue;
        };
        let mut may = starts[block]
            .clone()
            .expect("reachable blocks have a start");
        for (effect, live_after) in flow.instructions.iter().zip(&flow.live_after) {
            step(effect, live_after, &mut may);
        }
        let before_terminator = may.clone();
        for root in &flow.terminator.moves {
            may.remove(*root);
        }
        exits[block] = Some(before_terminator);
        for &successor in &graph.successors[block] {
            let mut dying = may.clone();
            if let Some(binding) = edge_def(block, successor) {
                dying.insert(binding);
            }
            dying.subtract(live_ins[successor].as_ref().expect("reachable successor"));
            if dying.is_empty() || doomed[successor] {
                continue;
            }
            let from_for = matches!(
                function.blocks[block].terminator,
                Terminator::ForRange { .. }
            );
            if graph.incoming(successor) == 1 || from_for {
                start_releases[successor].union_with(&dying);
                continue;
            }
            let mut ordinal = edge_blocks.len();
            let label = loop {
                let label = format!("%edge{ordinal}");
                if labels.insert(label.clone()) {
                    break label;
                }
                ordinal += 1;
            };
            edge_blocks.push(BasicBlock {
                label: label.clone(),
                instructions: releases(&facts, &dying),
                terminator: Terminator::Goto(function.blocks[successor].label.clone()),
            });
            retargets[block].push((successor, label));
        }
    }

    // Emit the blocks.
    let mut next_temporary = function
        .local_types
        .iter()
        .filter_map(|local| local.name.strip_prefix("%t")?.parse::<usize>().ok())
        .max()
        .map_or(0, |max| max + 1);
    let mut locals = Vec::new();
    let mut blocks = Vec::with_capacity(function.blocks.len() + edge_blocks.len());
    for (index, block) in function.blocks.iter().enumerate() {
        let Some(flow) = flows[index].as_ref() else {
            blocks.push(block.clone());
            continue;
        };
        let mut output = releases(&facts, &start_releases[index]);
        let mut may = starts[index]
            .clone()
            .expect("reachable blocks have a start");
        let mut deferred = RootSet::new(width);
        for (position, (instruction, effect)) in block
            .instructions
            .iter()
            .zip(&flow.instructions)
            .enumerate()
        {
            for root in &effect.moves {
                may.remove(*root);
            }
            let mut early = RootSet::new(width);
            let mut through_temporary = None;
            for &root in &effect.assigns {
                if !may.contains(root) {
                    continue;
                }
                match instruction {
                    Instruction::Assign { target, .. }
                        if effect.uses.contains(&root) && through_temporary.is_none() =>
                    {
                        let temporary = format!("%t{next_temporary}");
                        next_temporary += 1;
                        locals.push(MirLocalType {
                            name: temporary.clone(),
                            ty: facts.root_type(root).unwrap_or(Type::Unit),
                        });
                        through_temporary = Some((target.clone(), temporary));
                    }
                    _ => early.insert(root),
                }
            }
            output.extend(releases(&facts, &early));
            may.subtract(&early);
            for place in &effect.retains {
                output.push(Instruction::Retain {
                    place: (*place).to_string(),
                });
            }
            let mut emitted = instruction.clone();
            set_try_releases(&mut emitted, || {
                may.difference(&facts.kept())
                    .iter()
                    .map(|root| facts.roots[root].to_string())
                    .collect()
            });
            match through_temporary {
                Some((target, temporary)) => {
                    let Instruction::Assign { value, .. } = emitted else {
                        unreachable!("only an assignment goes through a temporary")
                    };
                    output.push(Instruction::Assign {
                        target: temporary.clone(),
                        value,
                    });
                    output.push(Instruction::Release {
                        place: target.clone(),
                    });
                    output.push(Instruction::Assign {
                        target,
                        value: Rvalue::Use(Operand::MovePlace(temporary)),
                    });
                }
                None => output.push(emitted),
            }
            for root in &effect.defs {
                may.insert(*root);
            }
            let mut dying = may.difference(&flow.live_after[position]);
            may.subtract(&dying);
            dying.union_with(&deferred);
            if doomed[index] {
                deferred = RootSet::new(width);
            } else if glued(&block.instructions, position) {
                deferred = dying;
            } else {
                deferred = RootSet::new(width);
                output.extend(releases(&facts, &dying));
            }
        }
        output.extend(releases(&facts, &deferred));
        let mut terminator = block.terminator.clone();
        if let Terminator::Return(value) = &terminator {
            let mut left = exits[index].clone().expect("reachable blocks have an exit");
            left.subtract(&facts.kept());
            if let Operand::Place(place) | Operand::MovePlace(place) = value {
                if let Some(root) = facts.tracked(root_of(place)) {
                    left.remove(root);
                }
            }
            output.extend(releases(&facts, &left));
        }
        for place in &flow.terminator.retains {
            output.push(Instruction::Retain {
                place: (*place).to_string(),
            });
        }
        for (successor, label) in &retargets[index] {
            retarget(&mut terminator, &function.blocks[*successor].label, label);
        }
        blocks.push(BasicBlock {
            label: block.label.clone(),
            instructions: output,
            terminator,
        });
    }
    blocks.extend(edge_blocks);
    Ok(Some(FunctionOwnership { blocks, locals }))
}

/// Moves `held` past one instruction: moves empty roots, stores fill
/// them, and dead roots are released after it.
fn step(effect: &Effect<'_>, live_after: &RootSet, held: &mut RootSet) {
    for root in &effect.moves {
        held.remove(*root);
    }
    for root in &effect.defs {
        held.insert(*root);
    }
    held.intersect_with(live_after);
}

/// `Release` instructions for `roots`, in name order.
fn releases(facts: &OwnershipFacts<'_>, roots: &RootSet) -> Vec<Instruction> {
    // The caller keeps a borrowed parameter, so no path releases it.
    roots
        .iter()
        .filter(|root| !facts.borrowed.contains(*root))
        .map(|root| Instruction::Release {
            place: facts.roots[root].to_string(),
        })
        .collect()
}

/// Fills the error-edge releases of a `try` instruction.
fn set_try_releases(instruction: &mut Instruction, releases: impl FnOnce() -> Vec<String>) {
    if let Instruction::Assign {
        value: Rvalue::Try {
            releases_on_error, ..
        },
        ..
    }
    | Instruction::WriteLoan {
        value: Rvalue::Try {
            releases_on_error, ..
        },
        ..
    } = instruction
    {
        *releases_on_error = releases();
    }
}

/// Whether nothing may come between instruction `position` and the next
/// one: a returned-view loan must follow its call at once. An
/// `IndexInRange` fact needs no glue, because the accesses it covers say
/// so themselves and a release between them keeps the fact (see
/// `mir_checks.rs`).
fn glued(instructions: &[Instruction], position: usize) -> bool {
    let Some(next) = instructions.get(position + 1) else {
        return false;
    };
    matches!(
        instructions[position],
        Instruction::Assign {
            value: Rvalue::Call { .. },
            ..
        }
    ) && matches!(next, Instruction::BeginReturnedLoan { .. })
}

/// Points every edge of `terminator` to `from` at `to` instead.
fn retarget(terminator: &mut Terminator, from: &str, to: &str) {
    relabel(terminator, |label| {
        if label == from {
            *label = to.to_string();
        }
    });
}

/// Applies `relabel` to every label `terminator` names.
fn relabel(terminator: &mut Terminator, mut relabel: impl FnMut(&mut String)) {
    match terminator {
        Terminator::Goto(label) => relabel(label),
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => {
            relabel(then_label);
            relabel(else_label);
        }
        Terminator::ForRange {
            body_label,
            exit_label,
            ..
        } => {
            relabel(body_label);
            relabel(exit_label);
        }
        Terminator::Match {
            arms, otherwise, ..
        } => {
            for arm in arms {
                relabel(&mut arm.label);
            }
            relabel(otherwise);
        }
        Terminator::Return(_) | Terminator::AssertFail { .. } | Terminator::Unreachable => {}
    }
}

/// What a backend needs to know about the locals of one function in an
/// explicit module.
#[derive(Clone, Debug, Default)]
pub(crate) struct ExplicitLocals {
    /// The roots whose values the module retains and releases.
    pub(crate) tracked: HashSet<String>,
    /// The loans, which borrow and own nothing.
    pub(crate) loans: HashSet<String>,
    /// The match blocks whose fall-through edge the module treats as
    /// unreachable, because their chain covers every variant.
    pub(crate) closed_matches: HashSet<String>,
}

/// The locals each function of an explicit module states ownership for, by
/// function name. An implicit module states none, so the map is empty.
pub(crate) fn explicit_locals(module: &MirModule) -> HashMap<String, ExplicitLocals> {
    if module.ownership != MirOwnership::Explicit {
        return HashMap::new();
    }
    let Ok(context) = MirLoanValidationContext::new(module) else {
        return HashMap::new();
    };
    module
        .functions
        .iter()
        .chain(module.top_level.iter())
        .map(|function| {
            let facts = OwnershipFacts::new(function, &context);
            let locals = ExplicitLocals {
                tracked: facts.roots.iter().map(|root| root.to_string()).collect(),
                loans: facts.loans.iter().map(|loan| loan.to_string()).collect(),
                closed_matches: FlowGraph::new(function, &context)
                    .map(|graph| {
                        graph
                            .closed
                            .iter()
                            .map(|block| function.blocks[*block].label.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            (function.name.clone(), locals)
        })
        .collect()
}

/// Removes what the ownership pass added: the retains, the releases, the
/// `try` releases, and the edge blocks that hold only releases. A retain
/// the pass cancelled into a move stays a move. Tests use it to forge
/// implicit MIR.
pub fn strip_ownership(module: &mut MirModule) {
    if module.ownership == MirOwnership::Implicit {
        return;
    }
    for function in module
        .functions
        .iter_mut()
        .chain(module.top_level.iter_mut())
    {
        super::lending::unlend_function(function);
        let mut forwards = HashMap::new();
        for block in &function.blocks {
            if let (true, Terminator::Goto(target)) = (
                block.label.starts_with("%edge")
                    && block
                        .instructions
                        .iter()
                        .all(|instruction| matches!(instruction, Instruction::Release { .. })),
                &block.terminator,
            ) {
                forwards.insert(block.label.clone(), target.clone());
            }
        }
        function
            .blocks
            .retain(|block| !forwards.contains_key(&block.label));
        for block in &mut function.blocks {
            block.instructions.retain(|instruction| {
                !matches!(
                    instruction,
                    Instruction::Retain { .. } | Instruction::Release { .. }
                )
            });
            for instruction in &mut block.instructions {
                set_try_releases(instruction, Vec::new);
            }
            // An edge block never leads to another one: the pass makes one
            // block per edge.
            relabel(&mut block.terminator, |label| {
                if let Some(target) = forwards.get(label.as_str()) {
                    *label = target.clone();
                }
            });
        }
    }
    module.ownership = MirOwnership::Implicit;
    module.borrowing.clear();
}

#[cfg(test)]
#[path = "mir_ownership_tests.rs"]
mod tests;
