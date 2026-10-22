use super::{operand_roles, terminator_roles, OperandRole};
use crate::ast::{BinaryOp, ReceiverKind, UnaryOp};
use crate::diag::Span;
use crate::mir::{
    AssertionCapture, CallTarget, Instruction, MirArg, MirClosureCapture, MirExternCall,
    MirFieldInit, MirFormatPart, MirMapEntry, MirReceiverKind, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{DictOp, FunctionOp, ListOp, RuntimeOp};
use crate::sema::Type;
use OperandRole::{Borrow, Consume};

fn place(name: &str) -> Operand {
    Operand::Place(name.to_string())
}

fn arg(name: Option<&str>, value: &str) -> MirArg {
    MirArg {
        name: name.map(str::to_string),
        value: place(value),
        writeback_place: None,
    }
}

fn assign(value: Rvalue) -> Instruction {
    Instruction::Assign {
        target: "t".to_string(),
        value,
    }
}

/// The roles of `instruction`, by the place each operand names.
fn roles(instruction: &Instruction) -> Vec<(String, OperandRole)> {
    operand_roles(instruction)
        .into_iter()
        .map(|(operand, role)| (named(operand), role))
        .collect()
}

fn named(operand: &Operand) -> String {
    match operand {
        Operand::Place(name) | Operand::MovePlace(name) => name.clone(),
        other => format!("{other:?}"),
    }
}

fn expect(pairs: &[(&str, OperandRole)]) -> Vec<(String, OperandRole)> {
    pairs
        .iter()
        .map(|(name, role)| (name.to_string(), *role))
        .collect()
}

fn call(callee: CallTarget, args: Vec<MirArg>) -> Instruction {
    assign(Rvalue::Call { callee, args })
}

#[test]
fn an_argument_role_follows_its_passing() {
    assert_eq!(OperandRole::of_passing(ReceiverKind::Value), Consume);
    assert_eq!(OperandRole::of_passing(ReceiverKind::Borrow), Borrow);
    assert_eq!(OperandRole::of_passing(ReceiverKind::BorrowMut), Borrow);
}

#[test]
fn instructions_without_operands_have_no_roles() {
    for instruction in [
        Instruction::Safepoint,
        Instruction::Retain {
            place: "a".to_string(),
        },
        Instruction::Release {
            place: "a".to_string(),
        },
        Instruction::IndexInRange {
            list: "a".to_string(),
            index: "i".to_string(),
        },
        Instruction::EndLoan {
            loan: "l".to_string(),
        },
        Instruction::PopCleanup {
            place: "a".to_string(),
            cancel_before_cleanup: false,
            run: true,
        },
        assign(Rvalue::UnionTagTest {
            place: "a".to_string(),
            union_type: Type::Unit,
            member_index: 0,
        }),
        assign(Rvalue::ModuleConstant {
            key: "k".to_string(),
            initializer: "i".to_string(),
        }),
    ] {
        assert!(roles(&instruction).is_empty(), "{instruction:?}");
    }
}

#[test]
fn a_target_takes_what_it_is_given() {
    assert_eq!(
        roles(&assign(Rvalue::Use(place("a")))),
        expect(&[("a", Consume)])
    );
    assert_eq!(
        roles(&Instruction::WriteLoan {
            loan: "l".to_string(),
            value: Rvalue::Use(place("a")),
        }),
        expect(&[("a", Consume)])
    );
    assert_eq!(
        roles(&assign(Rvalue::Try {
            value: place("r"),
            releases_on_error: Vec::new(),
        })),
        expect(&[("r", Consume)])
    );
    assert_eq!(
        roles(&assign(Rvalue::UnionInject {
            value: place("a"),
            union_type: Type::Unit,
            member_type: Type::Unit,
            member_index: 0,
        })),
        expect(&[("a", Consume)])
    );
    assert_eq!(
        roles(&Instruction::BeginElementLoan {
            loan: "l".to_string(),
            source: "items".to_string(),
            selector: place("key"),
            projection: String::new(),
            mutable: false,
            span: Span::new(1, 1),
        }),
        expect(&[("key", Consume)]),
        "the loan owns its selector"
    );
}

#[test]
fn reads_and_projections_borrow() {
    let span = Span::new(1, 1);
    assert_eq!(
        roles(&Instruction::Eval { value: place("a") }),
        expect(&[("a", Borrow)])
    );
    assert_eq!(
        roles(&assign(Rvalue::Binary {
            op: BinaryOp::Add,
            left: place("a"),
            right: place("b"),
            span,
        })),
        expect(&[("a", Borrow), ("b", Borrow)])
    );
    assert_eq!(
        roles(&assign(Rvalue::Unary {
            op: UnaryOp::Neg,
            value: place("a"),
            span,
        })),
        expect(&[("a", Borrow)])
    );
    assert_eq!(
        roles(&assign(Rvalue::Member {
            object: place("a"),
            field: "x".to_string(),
        })),
        expect(&[("a", Borrow)])
    );
    assert_eq!(
        roles(&assign(Rvalue::FormatString {
            parts: vec![
                MirFormatPart::Literal("x".to_string()),
                MirFormatPart::Value(place("a")),
                MirFormatPart::Int64(place("n")),
            ],
        })),
        expect(&[("a", Borrow), ("n", Borrow)])
    );
    assert_eq!(
        roles(&assign(Rvalue::StartTask {
            returns_handle: true,
            result_is_copy: false,
            stack_size: Some(place("size")),
            task_group: place("group"),
            function: place("worker"),
            args: vec![arg(None, "a")],
            span,
        })),
        expect(&[
            ("size", Borrow),
            ("group", Borrow),
            ("worker", Borrow),
            ("a", Borrow)
        ]),
        "the runtime copies a task's inputs across"
    );
}

#[test]
fn aggregates_consume_their_parts() {
    assert_eq!(
        roles(&assign(Rvalue::VecLiteral {
            elements: vec![place("a"), place("b")],
            element_type: Type::Unit,
        })),
        expect(&[("a", Consume), ("b", Consume)])
    );
    assert_eq!(
        roles(&assign(Rvalue::MapLiteral {
            entries: vec![MirMapEntry {
                key: place("k"),
                value: place("v"),
            }],
            key_type: Type::Unit,
            value_type: Type::Unit,
        })),
        expect(&[("k", Consume), ("v", Consume)])
    );
    assert_eq!(
        roles(&assign(Rvalue::Construct {
            class_name: "C".to_string(),
            fields: vec![MirFieldInit {
                name: "x".to_string(),
                value: place("a"),
            }],
        })),
        expect(&[("a", Consume)])
    );
    assert_eq!(
        roles(&assign(Rvalue::EnumVariant {
            enum_name: "E".to_string(),
            variant_name: "V".to_string(),
            payloads: vec![place("a")],
        })),
        expect(&[("a", Consume)])
    );
    let capture = |name: &str, passing| MirClosureCapture {
        name: name.to_string(),
        value: place(name),
        ty: Type::Unit,
        passing,
        mutated: false,
        source_place: None,
        resolve_source_at_capture: false,
    };
    assert_eq!(
        roles(&assign(Rvalue::Closure {
            function: "f".to_string(),
            signature: Type::Unit,
            captures: vec![
                capture("owned", MirReceiverKind::Value),
                capture("shared", MirReceiverKind::Borrow),
                capture("lent", MirReceiverKind::BorrowMut),
            ],
            consuming: false,
            mutable: false,
        })),
        expect(&[("owned", Consume), ("shared", Borrow), ("lent", Borrow)])
    );
}

#[test]
fn calls_consume_arguments_except_extern_and_runtime_borrows() {
    assert_eq!(
        roles(&call(
            CallTarget::Name("f".to_string()),
            vec![arg(None, "a")]
        )),
        expect(&[("a", Consume)])
    );
    assert_eq!(
        roles(&call(CallTarget::Value(place("f")), vec![arg(None, "a")])),
        expect(&[("f", Borrow), ("a", Consume)])
    );
    assert_eq!(
        roles(&call(
            CallTarget::Member {
                object: place("o"),
                field: "m".to_string(),
                receiver_place: Some("o".to_string()),
            },
            vec![arg(None, "a")]
        )),
        expect(&[("o", Consume), ("a", Consume)]),
        "the callee owns its receiver too"
    );
    assert_eq!(
        roles(&call(
            CallTarget::Extern(MirExternCall {
                symbol: "c".to_string(),
                abi: "C".to_string(),
                params: Vec::new(),
                return_type: Type::Unit,
            }),
            vec![arg(None, "a")]
        )),
        expect(&[("a", Borrow)])
    );
    let runtime = |op, receiver: Option<&str>, args| {
        call(
            CallTarget::Runtime {
                op,
                receiver: receiver.map(place),
                receiver_place: receiver.map(str::to_string),
            },
            args,
        )
    };
    assert_eq!(
        roles(&runtime(
            RuntimeOp::List(ListOp::Push),
            Some("items"),
            vec![arg(None, "a")]
        )),
        expect(&[("items", Borrow), ("a", Consume)])
    );
    // Named arguments bind by name, whatever their order.
    assert_eq!(
        roles(&runtime(
            RuntimeOp::Dict(DictOp::SetIndex),
            Some("d"),
            vec![
                arg(Some("line"), "line"),
                arg(Some("value"), "v"),
                arg(Some("key"), "k"),
                arg(Some("column"), "column"),
                arg(Some("missing"), "m"),
            ]
        )),
        expect(&[
            ("d", Borrow),
            ("line", Borrow),
            ("v", Consume),
            ("k", Consume),
            ("column", Borrow),
            ("m", Borrow)
        ])
    );
    assert_eq!(
        roles(&runtime(
            RuntimeOp::Function(FunctionOp::Select),
            None,
            vec![arg(None, "a"), arg(None, "b")]
        )),
        expect(&[("a", Borrow), ("b", Borrow)])
    );
}

#[test]
fn terminators_return_by_consuming_and_test_by_borrowing() {
    let roles = |terminator: &Terminator| {
        terminator_roles(terminator)
            .into_iter()
            .map(|(operand, role)| (named(operand), role))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        roles(&Terminator::Return(place("a"))),
        expect(&[("a", Consume)])
    );
    assert!(roles(&Terminator::Goto("b".to_string())).is_empty());
    assert!(roles(&Terminator::Unreachable).is_empty());
    assert_eq!(
        roles(&Terminator::Branch {
            condition: place("c"),
            then_label: "t".to_string(),
            else_label: "e".to_string(),
        }),
        expect(&[("c", Borrow)])
    );
    assert_eq!(
        roles(&Terminator::ForRange {
            binding: "i".to_string(),
            iterable: place("r"),
            body_label: "b".to_string(),
            exit_label: "e".to_string(),
        }),
        expect(&[("r", Borrow)])
    );
    assert_eq!(
        roles(&Terminator::Match {
            scrutinee: place("s"),
            arms: Vec::new(),
            otherwise: "o".to_string(),
        }),
        expect(&[("s", Borrow)])
    );
    assert_eq!(
        roles(&Terminator::AssertFail {
            message: Some(place("m")),
            captures: vec![AssertionCapture {
                label: "x".to_string(),
                ty: Type::Unit,
                value: place("x"),
            }],
            span: Span::new(1, 1),
        }),
        expect(&[("m", Borrow), ("x", Borrow)])
    );
}
