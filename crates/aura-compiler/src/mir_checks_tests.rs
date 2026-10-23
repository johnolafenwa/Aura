use super::super::{
    ArithmeticTrap, BasicBlock, CallTarget, Instruction, MirFunction, MirModule, Operand, Rvalue,
};
use super::{binary_trap, unary_trap, validate_checks, NumericKind};
use crate::ast::{BinaryOp, UnaryOp};
use crate::runtime_ops::{ListOp, RuntimeOp, ScalarElement};

/// Loops whose facts cover a scalar read, a boxed read, shared and
/// mutable element loans, and a lent read, next to a read and arithmetic
/// no fact covers.
const LOOPS: &str = r#"
class Point:
    n: int64 = 0

class Store:
    docs: list[str] = []

    def scan(self) -> int64:
        mut sum = 0
        for doc in self.docs:
            sum += doc.len()
        return sum

def total(values: list[int64]) -> int64:
    mut sum = 0
    for value in values:
        sum += value
    return sum

def main():
    numbers: list[int64] = [1, 2, 3]
    print(total(numbers))
    names = ["a", "bc"]
    mut kept: list[str] = []
    for name in names:
        kept.append(name.clone())
    print(kept.len())
    mut letters = 0
    for name in names:
        letters += name.len()
    print(letters)
    mut points = [Point(), Point()]
    for point in mut points:
        point.n += 1
    print(points[0].n + points[1].n)
    mut store = Store()
    store.docs.append("xyz")
    print(store.scan())
    print(numbers[2])
"#;

/// Arithmetic on every kind the table distinguishes.
const ARITHMETIC: &str = r#"
def main():
    a: int64 = 7
    b: uint64 = 7
    c: int32 = 7
    d: float64 = 7.5
    e: int8 = 7
    print(a + 1)
    print(a // 2)
    print(a % 2)
    print(b // 2)
    print(c // 2)
    print(c % 2)
    print(-a)
    print(-c)
    print(d / 2.0)
    print(d // 2.0)
    print(d + 1.0)
    print(a ** 2)
    print(a << 1)
    print(e + 1)
    print(a < 9)
    print(-d)
"#;

fn lowered(source: &str) -> MirModule {
    crate::lower_source_to_mir(source).expect("the program should lower")
}

fn function<'a>(module: &'a MirModule, name: &str) -> &'a MirFunction {
    module
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn function_mut<'a>(module: &'a mut MirModule, name: &str) -> &'a mut MirFunction {
    module
        .functions
        .iter_mut()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn instructions(function: &MirFunction) -> impl Iterator<Item = &Instruction> {
    function.blocks.iter().flat_map(|block| &block.instructions)
}

fn list_op(instruction: &Instruction) -> Option<ListOp> {
    match instruction {
        Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::List(op),
                            ..
                        },
                    ..
                },
            ..
        } => Some(*op),
        _ => None,
    }
}

fn trap(instruction: &Instruction) -> Option<(BinaryOp, ArithmeticTrap)> {
    match instruction {
        Instruction::Assign {
            value: Rvalue::Binary { op, trap, .. },
            ..
        } => Some((*op, *trap)),
        _ => None,
    }
}

/// The block of `function` that holds a fact.
fn fact_block(function: &MirFunction) -> &BasicBlock {
    function
        .blocks
        .iter()
        .find(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::IndexInRange { .. }))
        })
        .expect("the loop should state a fact")
}

fn fact_position(instructions: &[Instruction]) -> usize {
    instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::IndexInRange { .. }))
        .expect("the block should state a fact")
}

#[test]
fn lowering_marks_what_each_fact_covers() {
    let module = lowered(LOOPS);
    // A scalar read and its advance.
    let block = fact_block(function(&module, "total"));
    let at = fact_position(&block.instructions);
    assert_eq!(
        list_op(&block.instructions[at + 1]),
        Some(ListOp::IndexScalarUnchecked(ScalarElement::Int64))
    );
    assert_eq!(
        trap(&block.instructions[at + 2]),
        Some((BinaryOp::Add, ArithmeticTrap::None))
    );
    // A boxed read, and a lent one whose fact names the temporary.
    let main = function(&module, "main");
    assert!(
        instructions(main).any(|instruction| list_op(instruction) == Some(ListOp::IndexUnchecked))
    );
    let scan = fact_block(function(&module, "Store.scan"));
    let at = fact_position(&scan.instructions);
    assert!(matches!(
        scan.instructions[at - 1],
        Instruction::Lend { .. }
    ));
    assert_eq!(
        list_op(&scan.instructions[at + 1]),
        Some(ListOp::IndexUnchecked)
    );
    // An element loan of a mutable loop.
    assert!(instructions(main).any(|instruction| matches!(
        instruction,
        Instruction::BeginElementLoan {
            checked: false,
            mutable: true,
            ..
        }
    )));
    // A read no fact covers stays checked, and so does every other add.
    assert!(
        instructions(main)
            .any(|instruction| list_op(instruction)
                == Some(ListOp::IndexScalar(ScalarElement::Int64)))
    );
    let unchecked_adds = module
        .functions
        .iter()
        .flat_map(instructions)
        .filter(|instruction| trap(instruction) == Some((BinaryOp::Add, ArithmeticTrap::None)))
        .count();
    assert_eq!(unchecked_adds, 3, "one advance per loop that reads");
    validate_checks(&module).expect("lowered checks should validate");
}

#[test]
fn each_operation_names_the_trap_its_operands_need() {
    let module = lowered(ARITHMETIC);
    let traps = instructions(function(&module, "main"))
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Binary { op, trap, .. },
                ..
            } => Some(format!("{op:?} {trap:?}")),
            Instruction::Assign {
                value: Rvalue::Unary { op, trap, .. },
                ..
            } => Some(format!("{op:?} {trap:?}")),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        traps,
        [
            "Add Overflow",
            "FloorDiv DivisionByZeroOrOverflow",
            "Mod DivisionByZero",
            "FloorDiv DivisionByZero",
            "FloorDiv DivisionByZeroOrOverflow",
            "Mod DivisionByZero",
            "Neg Overflow",
            "Neg Overflow",
            "Div DivisionByZero",
            "FloorDiv Runtime",
            "Add None",
            "Pow Runtime",
            "Shl Runtime",
            "Add Runtime",
            "Less None",
            "Neg None",
        ]
    );
    validate_checks(&module).expect("lowered traps should validate");
}

#[test]
fn the_trap_table_covers_every_operator() {
    use NumericKind as K;
    assert_eq!(
        binary_trap(BinaryOp::Eq, K::Other, K::Int64),
        ArithmeticTrap::Runtime
    );
    assert_eq!(
        binary_trap(BinaryOp::BitXor, K::Int64, K::Int64),
        ArithmeticTrap::Runtime
    );
    assert_eq!(
        binary_trap(BinaryOp::Mul, K::Uint64, K::Uint64),
        ArithmeticTrap::Overflow
    );
    assert_eq!(
        binary_trap(BinaryOp::Mod, K::Float32, K::Float32),
        ArithmeticTrap::Runtime
    );
    assert_eq!(
        binary_trap(BinaryOp::Div, K::Int64, K::Int64),
        ArithmeticTrap::DivisionByZero
    );
    assert_eq!(
        binary_trap(BinaryOp::And, K::Bool, K::Bool),
        ArithmeticTrap::None
    );
    assert_eq!(
        binary_trap(BinaryOp::Add, K::Unit, K::Unit),
        ArithmeticTrap::None
    );
    assert_eq!(
        unary_trap(UnaryOp::BitNot, K::Int64),
        ArithmeticTrap::Runtime
    );
    assert_eq!(unary_trap(UnaryOp::Not, K::Other), ArithmeticTrap::Runtime);
    assert_eq!(unary_trap(UnaryOp::Not, K::Bool), ArithmeticTrap::None);
    assert_eq!(NumericKind::of_literal(1 << 40, Some(K::Uint64)), K::Uint64);
    assert_eq!(
        NumericKind::of_literal(u128::from(u64::MAX), Some(K::Int32)),
        K::Other
    );
    assert_eq!(NumericKind::of_literal(5, Some(K::Int32)), K::Int32);
    assert!(ArithmeticTrap::Runtime.checks_divisor() && ArithmeticTrap::Runtime.checks_overflow());
    assert!(!ArithmeticTrap::None.checks_divisor() && !ArithmeticTrap::Overflow.checks_divisor());
}

/// The checks validator's verdict on `LOOPS` after `edit` changes the
/// instructions of `name`'s fact block.
fn forged(name: &str, edit: impl Fn(&mut Vec<Instruction>)) -> Result<(), String> {
    let mut module = lowered(LOOPS);
    let function = function_mut(&mut module, name);
    let block = function
        .blocks
        .iter_mut()
        .find(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::IndexInRange { .. }))
        })
        .expect("the loop should state a fact");
    edit(&mut block.instructions);
    validate_checks(&module)
}

fn index_of(instructions: &[Instruction]) -> String {
    let Instruction::IndexInRange { index, .. } = &instructions[fact_position(instructions)] else {
        unreachable!("the position holds a fact");
    };
    index.clone()
}

#[test]
fn the_validator_refuses_unchecked_access_no_fact_covers() {
    // No fact at all.
    let error = forged("total", |instructions| {
        instructions.remove(fact_position(instructions));
    })
    .expect_err("an uncovered read should be refused");
    assert!(error.contains("reads `values["), "{error}");
    assert!(error.contains("no `IndexInRange` covers it"), "{error}");
    // A store to the index between the fact and the read.
    let error = forged("total", |instructions| {
        let at = fact_position(instructions);
        let index = index_of(instructions);
        instructions.insert(
            at + 1,
            Instruction::Assign {
                target: index,
                value: Rvalue::Use(Operand::Int(9)),
            },
        );
    })
    .expect_err("a read after the index changes should be refused");
    assert!(error.contains("no `IndexInRange` covers it"), "{error}");
    // A release of the list between the fact and the read.
    let error = forged("total", |instructions| {
        let at = fact_position(instructions);
        instructions.insert(
            at + 1,
            Instruction::Release {
                place: "values".to_string(),
            },
        );
    })
    .expect_err("a read of a released list should be refused");
    assert!(error.contains("no `IndexInRange` covers it"), "{error}");
    // A lend out of a place under the list's root.
    let error = forged("total", |instructions| {
        let at = fact_position(instructions);
        instructions.insert(
            at + 1,
            Instruction::Lend {
                place: "values.0".to_string(),
                temp: "%t99".to_string(),
                mutable: false,
            },
        );
    })
    .expect_err("a read of a list a lend changed should be refused");
    assert!(error.contains("no `IndexInRange` covers it"), "{error}");
    // An advance with no fact keeps its overflow check.
    let error = forged("total", |instructions| {
        let at = fact_position(instructions);
        instructions.insert(at + 1, Instruction::Safepoint);
    })
    .expect_err("an uncovered read and advance should be refused");
    assert!(error.contains("no `IndexInRange` covers it"), "{error}");
    let error = forged("total", |instructions| {
        let at = fact_position(instructions);
        instructions.insert(at + 2, Instruction::Safepoint);
    })
    .expect_err("an uncovered advance should be refused");
    assert!(
        error.contains("names trap `None`") && error.contains("need `Overflow`"),
        "{error}"
    );
    // An element loan with no fact.
    let mut module = lowered(LOOPS);
    let block = function_mut(&mut module, "main")
        .blocks
        .iter_mut()
        .find(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BeginElementLoan {
                        checked: false,
                        mutable: true,
                        ..
                    }
                )
            })
        })
        .expect("the mutable loop should lend unchecked");
    let at = fact_position(&block.instructions);
    block.instructions.remove(at);
    let error = validate_checks(&module).expect_err("an uncovered loan should be refused");
    assert!(error.contains("lends `points["), "{error}");
}

#[test]
fn the_validator_refuses_unchecked_forms_off_their_shape() {
    let mut module = lowered(LOOPS);
    let main = function_mut(&mut module, "main");
    // A plain read marked unchecked.
    for instruction in main
        .blocks
        .iter_mut()
        .flat_map(|block| &mut block.instructions)
    {
        if let Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::List(op @ ListOp::IndexScalar(_)),
                            ..
                        },
                    args,
                    ..
                },
            ..
        } = instruction
        {
            *op = op.unchecked().expect("a read has an unchecked form");
            args[0].value = Operand::Int(2);
        }
    }
    let error = validate_checks(&module).expect_err("a read of a literal index has no fact");
    assert!(error.contains("outside an element read"), "{error}");

    let mut module = lowered(LOOPS);
    let main = function_mut(&mut module, "main");
    for instruction in main
        .blocks
        .iter_mut()
        .flat_map(|block| &mut block.instructions)
    {
        if let Instruction::BeginElementLoan {
            projection,
            checked: false,
            ..
        } = instruction
        {
            *projection = "n".to_string();
        }
    }
    let error = validate_checks(&module).expect_err("a projected loan has no fact");
    assert!(error.contains("through a projection"), "{error}");

    let mut module = lowered(ARITHMETIC);
    let main = function_mut(&mut module, "main");
    for instruction in main
        .blocks
        .iter_mut()
        .flat_map(|block| &mut block.instructions)
    {
        if let Instruction::Assign {
            value:
                Rvalue::Binary {
                    op: BinaryOp::FloorDiv,
                    trap,
                    ..
                },
            ..
        } = instruction
        {
            *trap = ArithmeticTrap::Overflow;
        }
    }
    let error = validate_checks(&module).expect_err("a division must check its divisor");
    assert!(error.contains("names trap `Overflow`"), "{error}");
}

#[test]
fn retains_releases_and_lends_may_sit_between_a_fact_and_its_access() {
    forged("total", |instructions| {
        let at = fact_position(instructions);
        for place in ["sum", "values"] {
            instructions.insert(
                at + 1,
                Instruction::Retain {
                    place: place.to_string(),
                },
            );
        }
        instructions.insert(
            at + 3,
            Instruction::Release {
                place: "sum".to_string(),
            },
        );
        // Between the read and the advance, the list may change.
        instructions.insert(
            at + 5,
            Instruction::Release {
                place: "values".to_string(),
            },
        );
    })
    .expect("the facts should still cover the read and the advance");
}

#[test]
fn the_interpreter_runs_unchecked_reads_as_checked_ones() {
    let module = lowered(LOOPS);
    let output = crate::mir_runtime::run(&module).expect("the loops should run");
    assert_eq!(output.stdout, "6\n2\n3\n2\n3\n3\n");
    // An explicit module must state its checks to the interpreter too.
    let mut module = lowered(LOOPS);
    let block = function_mut(&mut module, "total")
        .blocks
        .iter_mut()
        .find(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::IndexInRange { .. }))
        })
        .expect("the loop should state a fact");
    let at = fact_position(&block.instructions);
    block.instructions.remove(at);
    let error = crate::mir_runtime::run(&module).expect_err("an uncovered read should be refused");
    assert!(error.message.contains("invalid MIR checks"), "{error:?}");
}

#[test]
fn the_interpreter_lowering_leaves_checks_to_the_boundary() {
    let program = crate::check_source(ARITHMETIC).expect("the program should check");
    let implicit = crate::mir::lower_implicit(&program);
    let main = function(&implicit, "main");
    assert!(
        instructions(main)
            .all(|instruction| trap(instruction)
                .is_none_or(|(_, trap)| trap == ArithmeticTrap::None)),
        "`aura run` should not pay for the checks pass"
    );
    let program = crate::check_source(LOOPS).expect("the program should check");
    let implicit = crate::mir::lower_implicit(&program);
    assert!(
        implicit
            .functions
            .iter()
            .flat_map(instructions)
            .filter_map(list_op)
            .all(|op| !op.is_unchecked()),
        "an implicit module should mark no access"
    );
    // The native boundary gives an implicit module the checks pass, so it
    // emits the same object as the explicit one.
    let explicit = lowered(LOOPS);
    assert_eq!(
        crate::native_codegen::emit_host_object(&implicit),
        crate::native_codegen::emit_host_object(&explicit)
    );
}
