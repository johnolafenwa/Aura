//! Explicit access and arithmetic checks (Batch 7 step 8f).
//!
//! Every numeric `Binary` and `Unary` names its trap, so a backend emits
//! the check MIR names instead of choosing one. The operator and the
//! operand types decide the trap: see [`binary_trap`] and [`unary_trap`].
//!
//! An `IndexInRange { list, index }` fact proves `0 <= index <
//! list.len()`. The accesses it covers say so themselves:
//!
//! - an element read `list.__index(index, ..)` becomes
//!   `ListOp::IndexUnchecked` or `ListOp::IndexScalarUnchecked`;
//! - the advance `index = index + 1` has trap `None`, since `index` is
//!   below a list length;
//! - an element loan of `list[index]` has `checked: false`.
//!
//! A fact stays in force across retains, releases, lends, other facts,
//! and the element reads it covers. Any other instruction ends it, and so
//! does a release or a lend that names the list or the index. A read
//! needs the list unchanged since the fact, while an advance needs only
//! the index.
//!
//! `resolve_checks` names each trap and marks what each fact covers. It
//! runs just before the ownership pass: in `lower`, and at the native
//! boundary for an implicit module. `validate_checks`
//! runs at the native boundary and refuses a trap the table does not give
//! and an unchecked access no fact covers. The interpreter reads an
//! unchecked read as a checked one, and checks each fact.

use super::ownership::root_of;
use super::{
    ArithmeticTrap, BasicBlock, Instruction, MirArg, MirFunction, MirLoanValidationContext,
    MirModule, Operand, Rvalue,
};
use crate::ast::{BinaryOp, UnaryOp};
use crate::runtime_ops::RuntimeOp;
use crate::sema::Type;

/// The numeric layout class of an operand: one of the types MIR computes
/// inline, or `Other` for any value a runtime helper computes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericKind {
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float32,
    Float64,
    Bool,
    Unit,
    Other,
}

impl NumericKind {
    pub(crate) fn of(ty: &Type) -> Self {
        match ty {
            Type::Unit => Self::Unit,
            Type::Named(name, args) if args.is_empty() => match name.as_str() {
                "int8" => Self::Int8,
                "int16" => Self::Int16,
                "int32" => Self::Int32,
                "int64" => Self::Int64,
                "uint8" => Self::Uint8,
                "uint16" => Self::Uint16,
                "uint32" => Self::Uint32,
                "uint64" => Self::Uint64,
                "float32" => Self::Float32,
                "float64" => Self::Float64,
                "bool" => Self::Bool,
                _ => Self::Other,
            },
            _ => Self::Other,
        }
    }

    fn is_integer(self) -> bool {
        self.is_signed_integer()
            || matches!(
                self,
                Self::Uint8 | Self::Uint16 | Self::Uint32 | Self::Uint64
            )
    }

    fn is_signed_integer(self) -> bool {
        matches!(self, Self::Int8 | Self::Int16 | Self::Int32 | Self::Int64)
    }

    fn is_float(self) -> bool {
        matches!(self, Self::Float32 | Self::Float64)
    }

    /// Whether a value of this kind is one machine word that MIR computes
    /// on inline: a number or a `bool`.
    pub(crate) fn is_scalar(self) -> bool {
        !matches!(self, Self::Unit | Self::Other)
    }

    /// The kind an integer literal loads as: the `hint` when it fits,
    /// otherwise `int64` when it fits in 64 signed bits.
    fn of_literal(value: u128, hint: Option<Self>) -> Self {
        let fits = match hint {
            Some(Self::Uint64) => u64::try_from(value).is_ok(),
            Some(kind) if kind.is_integer() => i64::try_from(value).is_ok(),
            _ => false,
        };
        match hint {
            Some(kind) if fits => kind,
            _ if i64::try_from(value).is_ok() => Self::Int64,
            _ => Self::Other,
        }
    }
}

/// The trap of `left op right`. An operand MIR does not compute inline
/// goes to a runtime helper, and so do powers, shifts, bitwise operators
/// other than an integer `&`, `|`, or `^`, and float floor division and
/// remainder.
pub(crate) fn binary_trap(op: BinaryOp, left: NumericKind, right: NumericKind) -> ArithmeticTrap {
    if left == NumericKind::Other || right == NumericKind::Other {
        return ArithmeticTrap::Runtime;
    }
    let integer = [left, right].into_iter().find(|kind| kind.is_integer());
    let float = left.is_float() || right.is_float();
    match op {
        // An integer `&`, `|`, or `^` of two values of one type stays in
        // that type.
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
            if integer.is_some() && left == right =>
        {
            ArithmeticTrap::None
        }
        BinaryOp::Pow
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => ArithmeticTrap::Runtime,
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul if integer.is_some() => {
            ArithmeticTrap::Overflow
        }
        // Signed floor division overflows on `MIN // -1`.
        BinaryOp::FloorDiv if integer.is_some_and(NumericKind::is_signed_integer) => {
            ArithmeticTrap::DivisionByZeroOrOverflow
        }
        BinaryOp::FloorDiv | BinaryOp::Mod if integer.is_some() => ArithmeticTrap::DivisionByZero,
        BinaryOp::FloorDiv | BinaryOp::Mod if float => ArithmeticTrap::Runtime,
        BinaryOp::Div if integer.is_some() || float => ArithmeticTrap::DivisionByZero,
        _ => ArithmeticTrap::None,
    }
}

/// The trap of `op value`. Integer negation overflows on `MIN`.
pub(crate) fn unary_trap(op: UnaryOp, value: NumericKind) -> ArithmeticTrap {
    match op {
        UnaryOp::BitNot if value.is_integer() => ArithmeticTrap::None,
        UnaryOp::BitNot => ArithmeticTrap::Runtime,
        _ if value == NumericKind::Other => ArithmeticTrap::Runtime,
        UnaryOp::Neg if value.is_integer() => ArithmeticTrap::Overflow,
        UnaryOp::Neg | UnaryOp::Not => ArithmeticTrap::None,
    }
}

impl MirLoanValidationContext<'_> {
    /// The trap the table gives `value`, an rvalue stored to `target`, or
    /// `None` when `value` names no trap.
    fn table_trap(
        &self,
        function: &MirFunction,
        target: &str,
        value: &Rvalue,
    ) -> Option<ArithmeticTrap> {
        let target_hint = || {
            self.place_type(function, target)
                .ok()
                .flatten()
                .map(|ty| NumericKind::of(&ty))
                .filter(|kind| kind.is_integer())
        };
        match value {
            Rvalue::Binary {
                op, left, right, ..
            } => {
                // An integer literal loads with the target's kind, or else
                // with the other operand's, as each backend loads it.
                let place_integer = |operand: &Operand| match operand {
                    Operand::Place(_) | Operand::MovePlace(_) => {
                        Some(self.numeric_kind(function, operand, None))
                            .filter(|kind| kind.is_integer())
                    }
                    _ => None,
                };
                let hint = |operand: &Operand, other: &Operand| {
                    target_hint()
                        .or_else(|| place_integer(operand))
                        .or_else(|| {
                            matches!(operand, Operand::Int(_))
                                .then(|| place_integer(other))
                                .flatten()
                        })
                };
                let left_kind = self.numeric_kind(function, left, hint(left, right));
                let right_kind = self.numeric_kind(function, right, hint(right, left));
                Some(binary_trap(*op, left_kind, right_kind))
            }
            Rvalue::Unary { op, value, .. } => {
                let kind = self.numeric_kind(function, value, target_hint());
                Some(unary_trap(*op, kind))
            }
            _ => None,
        }
    }

    fn numeric_kind(
        &self,
        function: &MirFunction,
        operand: &Operand,
        hint: Option<NumericKind>,
    ) -> NumericKind {
        match operand {
            Operand::Int(value) => NumericKind::of_literal(*value, hint),
            other => self
                .operand_type(function, other)
                .ok()
                .flatten()
                .map_or(NumericKind::Other, |ty| NumericKind::of(&ty)),
        }
    }
}

/// The target and the rvalue of an instruction that computes one.
fn computed(instruction: &Instruction) -> Option<(&str, &Rvalue)> {
    match instruction {
        Instruction::Assign { target, value } => Some((target, value)),
        Instruction::WriteLoan { loan, value } => Some((loan, value)),
        _ => None,
    }
}

/// A fact in force. `list` is `None` once the list may have changed, and
/// the fact then covers only an advance.
struct Fact<'a> {
    list: Option<&'a str>,
    index: &'a str,
}

/// What an instruction is to the facts in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Covered {
    /// An element read of a list a fact covers.
    Read,
    /// The advance of an index a fact covers.
    Advance,
    /// An element loan of a list element a fact covers.
    Loan,
    /// Nothing a fact covers.
    No,
}

/// The list and index an element read `target = list.__index(index, ..)`
/// reads, whatever op it names.
fn element_read(instruction: &Instruction) -> Option<(&str, &str)> {
    let Instruction::Assign {
        target,
        value: Rvalue::Call { callee, args, .. },
    } = instruction
    else {
        return None;
    };
    let parts = callee.member_parts()?;
    let (Operand::Place(list), "__index", None) = (parts.object, parts.field, parts.trait_name)
    else {
        return None;
    };
    let Some(MirArg {
        value: Operand::Place(index),
        ..
    }) = args.first()
    else {
        return None;
    };
    (target != list && target != index).then_some((list.as_str(), index.as_str()))
}

/// The index `index = index + 1` advances.
fn advance(instruction: &Instruction) -> Option<&str> {
    match instruction {
        Instruction::Assign {
            target,
            value:
                Rvalue::Binary {
                    op: BinaryOp::Add,
                    left: Operand::Place(left),
                    right: Operand::Int(1),
                    ..
                },
        } if target == left => Some(target),
        _ => None,
    }
}

/// The list and index of an element loan of `list[index]` that selects
/// the element itself.
fn element_loan(instruction: &Instruction) -> Option<(&str, &str)> {
    match instruction {
        Instruction::BeginElementLoan {
            source,
            selector: Operand::Place(index),
            projection,
            ..
        } if projection.is_empty() => Some((source, index)),
        _ => None,
    }
}

/// Calls `visit` with each instruction of `block` and what the facts in
/// force before it cover, then applies the instruction to the facts.
fn walk_facts<'a>(block: &'a BasicBlock, mut visit: impl FnMut(usize, &'a Instruction, Covered)) {
    let mut facts: Vec<Fact<'a>> = Vec::new();
    let covers = |facts: &[Fact<'_>], list: Option<&str>, index: &str| {
        facts
            .iter()
            .any(|fact| fact.index == index && (list.is_none() || fact.list == list))
    };
    for (position, instruction) in block.instructions.iter().enumerate() {
        let read =
            element_read(instruction).filter(|(list, index)| covers(&facts, Some(list), index));
        let covered = if read.is_some() {
            Covered::Read
        } else if advance(instruction).is_some_and(|index| covers(&facts, None, index)) {
            Covered::Advance
        } else if element_loan(instruction)
            .is_some_and(|(list, index)| covers(&facts, Some(list), index))
        {
            Covered::Loan
        } else {
            Covered::No
        };
        visit(position, instruction, covered);
        match instruction {
            Instruction::IndexInRange { list, index } => facts.push(Fact {
                list: Some(list),
                index,
            }),
            Instruction::Retain { .. } => {}
            Instruction::Release { place } => changes(&mut facts, place),
            Instruction::Lend { place, temp, .. } | Instruction::EndLend { place, temp, .. } => {
                changes(&mut facts, place);
                changes(&mut facts, temp);
            }
            Instruction::Assign { target, .. } if covered == Covered::Read => {
                changes(&mut facts, target)
            }
            _ => facts.clear(),
        }
    }
}

/// Applies a release, a lend, or a store that names `place`: the place
/// may hold another value after it.
fn changes(facts: &mut Vec<Fact<'_>>, place: &str) {
    let root = root_of(place);
    facts.retain(|fact| root_of(fact.index) != root);
    for fact in facts {
        if fact.list.is_some_and(|list| root_of(list) == root) {
            fact.list = None;
        }
    }
}

enum Rewrite {
    Trap(ArithmeticTrap),
    UncheckedRead(RuntimeOp),
    UncheckedLoan,
}

/// Names the trap of every numeric operation and marks each access a fact
/// covers. It runs just before the ownership pass.
pub(crate) fn resolve_checks(module: &mut MirModule) {
    let rewrites = {
        // A module the validator cannot read keeps its operations; the
        // validator reports what is wrong with it.
        let Ok(context) = MirLoanValidationContext::new(module) else {
            return;
        };
        collect_module_rewrites(module, Some(&context))
    };
    apply_rewrites(module, rewrites);
}

/// Marks each access a fact covers, and names no trap. The native boundary
/// runs it after runtime operations resolve, so a hand-built element read
/// spelled as a member call gets the unchecked form lowering gives it.
/// Marking is idempotent, and it only drops checks that a fact proves.
pub(crate) fn mark_covered_accesses(module: &mut MirModule) {
    let rewrites = collect_module_rewrites(module, None);
    apply_rewrites(module, rewrites);
}

fn collect_module_rewrites(
    module: &MirModule,
    context: Option<&MirLoanValidationContext<'_>>,
) -> Rewrites {
    let mut rewrites = Vec::new();
    for (function_index, function) in module.functions.iter().enumerate() {
        collect_rewrites(context, function, Some(function_index), &mut rewrites);
    }
    if let Some(top_level) = &module.top_level {
        collect_rewrites(context, top_level, None, &mut rewrites);
    }
    rewrites
}

fn apply_rewrites(module: &mut MirModule, rewrites: Rewrites) {
    for (function, block, position, rewrite) in rewrites {
        let function = match function {
            Some(index) => &mut module.functions[index],
            None => module
                .top_level
                .as_mut()
                .expect("a top-level rewrite has a top-level function"),
        };
        apply_rewrite(&mut function.blocks[block].instructions[position], rewrite);
    }
}

type Rewrites = Vec<(Option<usize>, usize, usize, Rewrite)>;

/// The rewrites of `function`: the marks of what each fact covers, and,
/// with a `context`, the trap of every other numeric operation.
fn collect_rewrites(
    context: Option<&MirLoanValidationContext<'_>>,
    function: &MirFunction,
    function_index: Option<usize>,
    rewrites: &mut Rewrites,
) {
    for (block_index, block) in function.blocks.iter().enumerate() {
        walk_facts(block, |position, instruction, covered| {
            let mut push =
                |rewrite| rewrites.push((function_index, block_index, position, rewrite));
            match covered {
                Covered::Read => {
                    if let Some(op) = unchecked_form(instruction) {
                        push(Rewrite::UncheckedRead(op));
                    }
                    return;
                }
                Covered::Advance => {
                    push(Rewrite::Trap(ArithmeticTrap::None));
                    return;
                }
                Covered::Loan => {
                    push(Rewrite::UncheckedLoan);
                    return;
                }
                Covered::No => {}
            }
            if let Some(trap) = context
                .zip(computed(instruction))
                .and_then(|(context, (target, value))| context.table_trap(function, target, value))
            {
                push(Rewrite::Trap(trap));
            }
        });
    }
}

/// The unchecked form of a list element read.
fn unchecked_form(instruction: &Instruction) -> Option<RuntimeOp> {
    match instruction {
        Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        super::CallTarget::Runtime {
                            op: RuntimeOp::List(op),
                            ..
                        },
                    ..
                },
            ..
        } => op.unchecked().map(RuntimeOp::List),
        _ => None,
    }
}

fn apply_rewrite(instruction: &mut Instruction, rewrite: Rewrite) {
    match (instruction, rewrite) {
        (
            Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. },
            Rewrite::Trap(new),
        ) => {
            if let Rvalue::Binary { trap, .. } | Rvalue::Unary { trap, .. } = value {
                *trap = new;
            }
        }
        (
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee: super::CallTarget::Runtime { op, .. },
                        ..
                    },
                ..
            },
            Rewrite::UncheckedRead(unchecked),
        ) => *op = unchecked,
        (Instruction::BeginElementLoan { checked, .. }, Rewrite::UncheckedLoan) => *checked = false,
        _ => {}
    }
}

/// Refuses a trap the table does not give, except `None` on an advance a
/// fact covers, and an unchecked read or loan that no fact covers.
pub(crate) fn validate_checks(module: &MirModule) -> std::result::Result<(), String> {
    let context = MirLoanValidationContext::new(module)?;
    for function in module.functions.iter().chain(module.top_level.iter()) {
        for block in &function.blocks {
            let mut result = Ok(());
            walk_facts(block, |_, instruction, covered| {
                if result.is_ok() {
                    result = validate_instruction(&context, function, instruction, covered);
                }
            });
            result?;
        }
    }
    Ok(())
}

fn validate_instruction(
    context: &MirLoanValidationContext<'_>,
    function: &MirFunction,
    instruction: &Instruction,
    covered: Covered,
) -> std::result::Result<(), String> {
    let uncovered = |access: &str, (list, index): (&str, &str)| {
        format!(
            "invalid MIR function `{}` {access} `{list}[{index}]` without a bounds check, but no `IndexInRange` covers it",
            function.name
        )
    };
    if let Some(op) = unchecked_read_op(instruction) {
        if covered != Covered::Read {
            return Err(match element_read(instruction) {
                Some(read) => uncovered("reads", read),
                None => format!(
                    "invalid MIR function `{}` reads with `{op}` outside an element read",
                    function.name
                ),
            });
        }
    }
    if let Instruction::BeginElementLoan { checked: false, .. } = instruction {
        if covered != Covered::Loan {
            return Err(match element_loan(instruction) {
                Some(loan) => uncovered("lends", loan),
                None => format!(
                    "invalid MIR function `{}` begins an element loan through a projection without a bounds check",
                    function.name
                ),
            });
        }
    }
    let Some((target, value)) = computed(instruction) else {
        return Ok(());
    };
    let (Rvalue::Binary { trap, .. } | Rvalue::Unary { trap, .. }) = value else {
        return Ok(());
    };
    let expected = context
        .table_trap(function, target, value)
        .expect("a numeric rvalue has a table trap");
    if *trap == expected || (*trap == ArithmeticTrap::None && covered == Covered::Advance) {
        return Ok(());
    }
    Err(format!(
        "invalid MIR function `{}` names trap `{trap:?}` for `{target}`, but its operands need `{expected:?}`",
        function.name
    ))
}

fn unchecked_read_op(instruction: &Instruction) -> Option<RuntimeOp> {
    match instruction {
        Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        super::CallTarget::Runtime {
                            op: op @ RuntimeOp::List(list_op),
                            ..
                        },
                    ..
                },
            ..
        } if list_op.is_unchecked() => Some(*op),
        _ => None,
    }
}

#[cfg(test)]
#[path = "mir_checks_tests.rs"]
mod tests;
