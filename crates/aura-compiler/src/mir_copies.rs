//! Copies MIR makes explicit so that a pass can remove them (Batch 7 step 3).
//!
//! A shared `for` loop over a local list lends each element when its body
//! only reads the element in ways both backends support in place: comparing
//! it with `==` or `!=`, measuring it with `len()`, or looking a key up in it
//! with `get()`. A loan read is an explicit `ReadLoan` copy. The pass below
//! removes a copy whose only use is a comparison, so the comparison reads the
//! loan directly.

use std::collections::{BTreeSet, HashMap};

use crate::ast::{AssignTarget, BinaryOp, BindingTarget, Expr, ExprKind, FormatPart, Stmt};
use crate::mir::{Instruction, MirFunction, Operand, Rvalue};
use crate::sema::Type;

/// Whether a list of `element` values can be lent to a loop body: the
/// element is a `str`, `list`, `dict`, or `set`, which the backends compare,
/// measure, and search in place.
pub(crate) fn lendable_loop_element(element: &Type) -> bool {
    match element {
        Type::Named(name, args) => match name.as_str() {
            "str" => args.is_empty(),
            "list" | "set" => args.len() == 1,
            "dict" => args.len() == 2,
            _ => false,
        },
        _ => false,
    }
}

/// Whether `element.field(...)` with `arity` arguments reads a lent list
/// element in place: `len()` on any lendable element, and `get(key)` on a
/// `dict`.
pub(crate) fn borrowed_element_method(element: &Type, field: &str, arity: usize) -> bool {
    match (field, arity) {
        ("len", 0) => lendable_loop_element(element),
        ("get", 1) => {
            matches!(element, Type::Named(name, args) if name == "dict" && args.len() == 2)
        }
        _ => false,
    }
}

/// Whether every use of `name` in `body` reads the element in place, so a
/// loop that lends each element to `name` copies none. A body that rebinds
/// `name`, captures it in a closure, or uses it any other way keeps the
/// copying read.
pub(crate) fn body_only_borrows(body: &[Stmt], name: &str, element: &Type) -> bool {
    body.iter()
        .all(|stmt| stmt_only_borrows(stmt, name, element))
}

fn stmt_only_borrows(stmt: &Stmt, name: &str, element: &Type) -> bool {
    let expr = |expr: &Expr| expr_only_borrows(expr, name, element);
    let body = |body: &[Stmt]| body_only_borrows(body, name, element);
    match stmt {
        Stmt::Assign(assign) => {
            let target = match &assign.target {
                AssignTarget::Name(target) => target != name,
                AssignTarget::Member { object, .. } => {
                    !crate::sema::expr_references_name(object, name)
                }
                AssignTarget::Index { object, index } => {
                    !crate::sema::expr_references_name(object, name) && expr(index)
                }
            };
            target && expr(&assign.value)
        }
        Stmt::View(view) => {
            view.name != name && !crate::sema::expr_references_name(&view.source, name)
        }
        Stmt::Destructure(destructure) => {
            !binding_target_names(&destructure.target).contains(name) && expr(&destructure.value)
        }
        Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Assert(assertion) => {
            expr(&assertion.condition) && assertion.message.as_ref().is_none_or(expr)
        }
        Stmt::Return(return_stmt) => return_stmt.value.as_ref().is_none_or(expr),
        Stmt::If(if_stmt) => {
            if_stmt
                .branches
                .iter()
                .all(|branch| expr(&branch.condition) && body(&branch.body))
                && if_stmt.else_body.as_deref().is_none_or(body)
        }
        Stmt::Match(match_stmt) => {
            expr(&match_stmt.scrutinee)
                && match_stmt
                    .arms
                    .iter()
                    .all(|arm| arm.guard.as_ref().is_none_or(expr) && body(&arm.body))
        }
        Stmt::For(for_stmt) => {
            !binding_target_names(&for_stmt.target).contains(name)
                && expr(&for_stmt.iterable)
                && body(&for_stmt.body)
        }
        Stmt::With(with_stmt) => {
            with_stmt.binding != name && expr(&with_stmt.value) && body(&with_stmt.body)
        }
        Stmt::While(while_stmt) => expr(&while_stmt.condition) && body(&while_stmt.body),
        Stmt::Expr(expr_stmt) => expr(&expr_stmt.expr),
    }
}

fn expr_only_borrows(expr: &Expr, name: &str, element: &Type) -> bool {
    let is_binding =
        |expr: &Expr| matches!(&grouped(expr).kind, ExprKind::Name(found) if found == name);
    let children = |exprs: &[&Expr]| {
        exprs
            .iter()
            .all(|child| expr_only_borrows(child, name, element))
    };
    match &expr.kind {
        ExprKind::Name(found) => found != name,
        ExprKind::Binary {
            op: BinaryOp::Eq | BinaryOp::NotEq,
            left,
            right,
        } => [left, right]
            .into_iter()
            .all(|side| is_binding(side) || expr_only_borrows(side, name, element)),
        ExprKind::Call { callee, args } => {
            let arguments_borrow = args
                .iter()
                .all(|argument| expr_only_borrows(&argument.value, name, element));
            match &grouped(callee).kind {
                ExprKind::Member { object, field }
                    if is_binding(object)
                        && borrowed_element_method(element, field, args.len()) =>
                {
                    arguments_borrow
                }
                _ => arguments_borrow && expr_only_borrows(callee, name, element),
            }
        }
        // A closure or comprehension that mentions the binding captures or
        // rebinds it, which needs a copy.
        ExprKind::Lambda { .. } | ExprKind::Comprehension { .. } => {
            !crate::sema::expr_references_name(expr, name)
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted => true,
        ExprKind::Group(inner) | ExprKind::Try(inner) => children(&[inner]),
        ExprKind::IsNone { value, .. }
        | ExprKind::Unary { expr: value, .. }
        | ExprKind::Cast { expr: value, .. }
        | ExprKind::Specialize { expr: value, .. }
        | ExprKind::Member { object: value, .. } => children(&[value]),
        ExprKind::Index { object, index } => children(&[object, index]),
        ExprKind::Slice {
            object, start, end, ..
        } => {
            children(&[object])
                && start.as_deref().is_none_or(|start| children(&[start]))
                && end.as_deref().is_none_or(|end| children(&[end]))
        }
        ExprKind::Binary { left, right, .. }
        | ExprKind::Membership {
            value: left,
            container: right,
            ..
        } => children(&[left, right]),
        ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => elements
            .iter()
            .all(|element_expr| children(&[element_expr])),
        ExprKind::Map(entries) => entries
            .iter()
            .all(|entry| children(&[&entry.key, &entry.value])),
        ExprKind::FString(parts) => parts.iter().all(|part| match part {
            FormatPart::Literal(_) => true,
            FormatPart::Expr(value) | FormatPart::Formatted { expr: value, .. } => {
                children(&[value])
            }
        }),
        ExprKind::Match {
            scrutinee, arms, ..
        } => {
            children(&[scrutinee])
                && arms.iter().all(|arm| {
                    arm.guard.as_ref().is_none_or(|guard| children(&[guard]))
                        && children(&[&arm.value])
                })
        }
        ExprKind::CompareChain { first, links } => {
            children(&[first]) && links.iter().all(|link| children(&[&link.operand]))
        }
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => children(&[then_expr, condition, else_expr]),
    }
}

fn grouped(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Group(inner) => grouped(inner),
        _ => expr,
    }
}

fn binding_target_names(target: &BindingTarget) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    let mut pending = vec![target];
    while let Some(target) = pending.pop() {
        match target {
            BindingTarget::Name { name, .. } | BindingTarget::Rest { name, .. } => {
                names.insert(name.as_str());
            }
            BindingTarget::Tuple { elements, .. } => pending.extend(elements),
        }
    }
    names
}

/// Removes each `ReadLoan` copy whose only use is an operand of a comparison
/// later in the same block, with nothing but other loan reads between them.
/// The comparison reads the loan instead, which both backends do in place:
/// the loan is still active, and a comparison never keeps its operands.
pub(crate) fn cancel_loan_copies(function: &mut MirFunction) {
    let uses = place_use_counts(function);
    let mut removed = BTreeSet::new();
    for block in &mut function.blocks {
        let mut index = 0;
        while index < block.instructions.len() {
            let Instruction::ReadLoan { target, loan } = &block.instructions[index] else {
                index += 1;
                continue;
            };
            if !target.starts_with("%t") || uses.get(target.as_str()) != Some(&1) {
                index += 1;
                continue;
            }
            let (target, loan) = (target.clone(), loan.clone());
            let consumer = block.instructions[index + 1..]
                .iter()
                .position(|instruction| !matches!(instruction, Instruction::ReadLoan { .. }))
                .map(|offset| index + 1 + offset);
            let replaced = consumer.is_some_and(|consumer| {
                replace_comparison_operand(&mut block.instructions[consumer], &target, &loan)
            });
            if replaced {
                block.instructions.remove(index);
                removed.insert(target);
            } else {
                index += 1;
            }
        }
    }
    function
        .local_types
        .retain(|local| !removed.contains(&local.name));
}

/// How many times each place root is read, plus one for each definition by
/// an `Assign` or a second `ReadLoan`, so a count of 1 for a `ReadLoan`
/// target means one definition and exactly one read.
fn place_use_counts(function: &MirFunction) -> HashMap<String, usize> {
    fn count(counts: &mut HashMap<String, usize>, place: &str) {
        let root = place.split('.').next().unwrap_or(place);
        *counts.entry(root.to_string()).or_default() += 1;
    }
    let mut counts = HashMap::new();
    let mut loan_reads = BTreeSet::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            let mut mentions = Vec::new();
            crate::mir::instruction_place_mentions(instruction, &mut mentions);
            match instruction {
                // The target is listed as a mention; its first definition is
                // not a read.
                Instruction::ReadLoan { target, .. } if loan_reads.insert(target.as_str()) => {
                    let position = mentions
                        .iter()
                        .position(|mention| *mention == target.as_str())
                        .expect("a loan read mentions its target");
                    mentions.remove(position);
                }
                Instruction::Assign { target, .. } => count(&mut counts, target),
                _ => {}
            }
            for mention in mentions {
                count(&mut counts, mention);
            }
        }
        for mention in crate::mir::terminator_place_mentions(&block.terminator) {
            count(&mut counts, mention);
        }
    }
    counts
}

/// Rewrites a comparison operand `Place(copy)` to read `loan`. Returns
/// whether `instruction` was that comparison.
fn replace_comparison_operand(instruction: &mut Instruction, copy: &str, loan: &str) -> bool {
    let Instruction::Assign {
        target,
        value:
            Rvalue::Binary {
                op:
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Less
                    | BinaryOp::LessEq
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEq,
                left,
                right,
                ..
            },
    } = instruction
    else {
        return false;
    };
    if target == copy {
        return false;
    }
    for operand in [left, right] {
        if matches!(operand, Operand::Place(place) if place == copy) {
            *operand = Operand::Place(loan.to_string());
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "mir_copies_tests.rs"]
mod tests;
