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

use super::{
    CallTarget, Instruction, MirArg, MirFormatPart, MirReceiverKind, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::RuntimeOp;

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

/// Every operand of `instruction` with its role, in evaluation order.
pub fn operand_roles(instruction: &Instruction) -> Vec<(&Operand, OperandRole)> {
    let mut roles = Vec::new();
    match instruction {
        Instruction::Safepoint
        | Instruction::Retain { .. }
        | Instruction::Release { .. }
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
        Rvalue::Call { callee, args } => call_roles(callee, args, roles),
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
                roles.push((&arg.value, runtime_arg_role(*op, arg, position)));
            }
            return;
        }
    }
    for arg in args {
        roles.push((&arg.value, Consume));
    }
}

/// The role of a runtime op's argument: the ownership of the parameter it
/// binds by name, or by position when it has no name. An argument the op
/// does not declare borrows, as a variadic `select` source does.
fn runtime_arg_role(op: RuntimeOp, arg: &MirArg, position: usize) -> OperandRole {
    let index = match &arg.name {
        Some(name) => op.params().iter().position(|param| param.name == name),
        None => Some(position),
    };
    index
        .and_then(|index| op.arg_ownership(index))
        .unwrap_or(OperandRole::Borrow)
}

#[cfg(test)]
#[path = "mir_ownership_tests.rs"]
mod tests;
