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

mod pass {
    use super::super::{insert_ownership, insert_ownership_within, strip_ownership, FlowGraph};
    use crate::mir::{
        validate_loan_flow, validate_ownership, BasicBlock, Instruction, MirFunction,
        MirLoanValidationContext, MirModule, MirOwnership, Operand, Rvalue, Terminator,
    };

    const PROGRAM: &str = r#"enum Shape:
    Circle(int64)
    Square(int64)

def parse(text: str) -> Result[int64, str]:
    if text == "4":
        return Result.Ok(4)
    return Result.Err(f"bad {text}")

def double(text: str) -> Result[int64, str]:
    zeta = f"z {text}"
    alpha = f"a {text}"
    value = try parse(text)
    print(alpha + zeta)
    return Result.Ok(value * 2)

def pick(flag: bool) -> str:
    a = f"a{flag}"
    b = f"b{flag}"
    if flag:
        print(a)
    else:
        print(b)
    return "done"

def swap(flag: bool) -> str:
    mut s = f"s{flag}"
    s = s + "x"
    return s

def take(text: own str) -> int64:
    return len(text)

def route(text: own str, flag: bool) -> int64:
    if flag:
        match parse(text):
            case Result.Ok(value):
                return value
            case Result.Err(error):
                print(error)
    return take(text)

def area(shape: Shape, note: str) -> int64:
    assert note != "", "note"
    match shape:
        case Shape.Circle(r):
            return r
        case Shape.Square(side):
            return side

def sum(values: list[str]) -> int64:
    mut total = 0
    for value in values:
        total = total + len(value)
    return total

def main():
    print(pick(true))
    print(swap(false))
    print(route("4", true))
    print(area(Shape.Circle(2), "n"))
    print(sum(["a", "bc"]))
    match double("4"):
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
"#;

    fn lowered() -> MirModule {
        crate::lower_source_to_mir(PROGRAM).expect("the program should lower")
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

    fn block<'a>(function: &'a MirFunction, label: &str) -> &'a BasicBlock {
        function
            .blocks
            .iter()
            .find(|block| block.label == label)
            .unwrap_or_else(|| panic!("block `{label}` should exist"))
    }

    fn released(instructions: &[Instruction]) -> Vec<&str> {
        instructions
            .iter()
            .filter_map(|instruction| match instruction {
                Instruction::Release { place } => Some(place.as_str()),
                _ => None,
            })
            .collect()
    }

    fn checks(module: &MirModule) {
        validate_loan_flow(module).expect("the pass output should keep its loan flow");
        validate_ownership(module).expect("the pass output should check");
    }

    /// The lowered module with what the pass added taken out again.
    fn implicit() -> MirModule {
        let mut module = lowered();
        strip_ownership(&mut module);
        module
    }

    fn json(module: &MirModule) -> serde_json::Value {
        serde_json::to_value(module).expect("MIR should serialize")
    }

    #[test]
    fn lowering_states_ownership_and_the_pass_is_idempotent() {
        let module = lowered();
        assert_eq!(module.ownership, MirOwnership::Explicit);
        checks(&module);
        let mut again = module.clone();
        insert_ownership(&mut again);
        assert_eq!(json(&again), json(&module));
        // Stripping undoes the pass, and the pass then does the same again.
        let mut stripped = module.clone();
        strip_ownership(&mut stripped);
        assert_eq!(stripped.ownership, MirOwnership::Implicit);
        let mut restripped = stripped.clone();
        strip_ownership(&mut restripped);
        assert_eq!(json(&restripped), json(&stripped));
        insert_ownership(&mut stripped);
        assert_eq!(json(&stripped), json(&module));
    }

    #[test]
    fn values_dying_together_are_released_in_name_order() {
        let module = lowered();
        let double = function(&module, "double");
        let instructions = &block(double, "entry").instructions;
        let concat = instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Binary { .. },
                        ..
                    }
                )
            })
            .expect("`double` adds the strings");
        assert_eq!(
            released(&instructions[concat + 1..concat + 3]),
            ["alpha", "zeta"]
        );
        // The `try` lists what it must release on its error edge, by name.
        let listed = instructions
            .iter()
            .find_map(|instruction| match instruction {
                Instruction::Assign {
                    value:
                        Rvalue::Try {
                            releases_on_error, ..
                        },
                    ..
                } => Some(releases_on_error.clone()),
                _ => None,
            })
            .expect("`double` holds a `try`");
        assert_eq!(listed, ["alpha", "zeta"]);
    }

    #[test]
    fn a_branch_releases_what_its_side_does_not_use() {
        let module = lowered();
        let pick = function(&module, "pick");
        let sides = pick
            .blocks
            .iter()
            .filter(|block| block.label.contains("if_then") || block.label.contains("if_else"))
            .map(|block| released(&block.instructions))
            .collect::<Vec<_>>();
        assert_eq!(sides, [vec!["b", "a"], vec!["a", "b"]]);
    }

    #[test]
    fn a_value_dying_on_a_critical_edge_gets_its_own_block() {
        // Send `pick`'s false side straight to the join, which then has two
        // ways in, so `a` dies on the edge itself. `b` now dies at once.
        let mut module = implicit();
        let pick = function_mut(&mut module, "pick");
        let join = pick
            .blocks
            .iter()
            .find(|block| matches!(block.terminator, Terminator::Return(_)))
            .unwrap()
            .label
            .clone();
        let entry = pick
            .blocks
            .iter_mut()
            .find(|block| block.label == "entry")
            .unwrap();
        let Terminator::Branch { else_label, .. } = &mut entry.terminator else {
            panic!("`pick` branches on its flag");
        };
        *else_label = join.clone();
        let forged = json(&module);
        insert_ownership(&mut module);
        checks(&module);
        let pick = function(&module, "pick");
        let edge = block(pick, "%edge0");
        assert_eq!(released(&edge.instructions), ["a"]);
        assert_eq!(released(&block(pick, "entry").instructions), ["b"]);
        assert!(matches!(&edge.terminator, Terminator::Goto(target) if *target == join));
        assert!(matches!(
            &block(pick, "entry").terminator,
            Terminator::Branch { else_label, .. } if else_label == "%edge0"
        ));
        // The edge block goes away again with the rest of the pass output.
        strip_ownership(&mut module);
        assert_eq!(json(&module), forged);
    }

    #[test]
    fn an_edge_block_takes_a_free_label() {
        let mut module = implicit();
        let pick = function_mut(&mut module, "pick");
        let join = pick
            .blocks
            .iter()
            .find(|block| matches!(block.terminator, Terminator::Return(_)))
            .unwrap()
            .label
            .clone();
        let entry = pick
            .blocks
            .iter_mut()
            .find(|block| block.label == "entry")
            .unwrap();
        let Terminator::Branch { else_label, .. } = &mut entry.terminator else {
            panic!("`pick` branches on its flag");
        };
        *else_label = join;
        // A stray block already holds the first edge label.
        pick.blocks.push(BasicBlock {
            label: "%edge0".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Unreachable,
        });
        insert_ownership(&mut module);
        checks(&module);
        let pick = function(&module, "pick");
        assert_eq!(released(&block(pick, "%edge1").instructions), ["a"]);
    }

    #[test]
    fn a_store_that_reads_its_target_goes_through_a_temporary() {
        let mut module = implicit();
        let swap = function_mut(&mut module, "swap");
        let instructions = &mut swap.blocks[0].instructions;
        let add = instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Binary { .. },
                        ..
                    }
                )
            })
            .unwrap();
        let Instruction::Assign { value, .. } = instructions.remove(add) else {
            unreachable!()
        };
        // `s = s + "x"` in one store.
        instructions[add] = Instruction::Assign {
            target: "s".to_string(),
            value,
        };
        insert_ownership(&mut module);
        checks(&module);
        let swap = function(&module, "swap");
        let instructions = &swap.blocks[0].instructions;
        let release = instructions
            .iter()
            .position(
                |instruction| matches!(instruction, Instruction::Release { place } if place == "s"),
            )
            .unwrap();
        let Instruction::Assign {
            target,
            value: Rvalue::Binary { .. },
        } = &instructions[release - 1]
        else {
            panic!("the sum goes to a temporary first");
        };
        assert!(matches!(
            &instructions[release + 1],
            Instruction::Assign { target: stored, value: Rvalue::Use(Operand::MovePlace(moved)) }
                if stored == "s" && moved == target
        ));
        assert!(swap.local_types.iter().any(|local| local.name == *target));
    }

    #[test]
    fn nothing_comes_between_an_index_proof_and_its_access() {
        let module = lowered();
        checks(&module);
        let sum = function(&module, "sum");
        let mut proofs = 0;
        for block in &sum.blocks {
            for (index, instruction) in block.instructions.iter().enumerate() {
                if matches!(instruction, Instruction::IndexInRange { .. }) {
                    proofs += 1;
                    for next in &block.instructions[index + 1..index + 3] {
                        assert!(
                            !matches!(
                                next,
                                Instruction::Release { .. } | Instruction::Retain { .. }
                            ),
                            "{:?}",
                            block.instructions
                        );
                    }
                }
            }
        }
        assert!(proofs > 0, "the loop should read with a proof");
    }

    #[test]
    fn blocks_that_end_the_program_release_nothing() {
        let module = lowered();
        for name in ["area", "pick"] {
            for block in &function(&module, name).blocks {
                if matches!(
                    block.terminator,
                    Terminator::Unreachable | Terminator::AssertFail { .. }
                ) {
                    assert!(released(&block.instructions).is_empty(), "{block:?}");
                }
            }
        }
    }

    #[test]
    fn an_exhaustive_match_does_not_fall_through() {
        // `route` moves `text` into the match and uses it again after the
        // `if`; only the match's impossible fall-through joins the two.
        let module = lowered();
        checks(&module);
        let route = function(&module, "route");
        let context = MirLoanValidationContext::new(&module).unwrap();
        let graph = FlowGraph::new(route, &context).unwrap();
        let (last, otherwise) = route
            .blocks
            .iter()
            .enumerate()
            .find_map(|(index, block)| match &block.terminator {
                Terminator::Match {
                    arms, otherwise, ..
                } if arms[0].variant_name.as_deref() == Some("Err") => Some((index, otherwise)),
                _ => None,
            })
            .expect("the chain ends with the `Err` arm");
        let otherwise = route
            .blocks
            .iter()
            .position(|block| block.label == *otherwise)
            .unwrap();
        assert!(!graph.successors[last].contains(&otherwise));
        assert_eq!(graph.successors[last].len(), 1);
    }

    #[test]
    fn a_wildcard_arm_covers_every_variant() {
        let mut module = implicit();
        let route = function_mut(&mut module, "route");
        let head = route
            .blocks
            .iter_mut()
            .find_map(|block| match &mut block.terminator {
                Terminator::Match { arms, .. } if arms[0].variant_name.as_deref() == Some("Ok") => {
                    arms[0].wildcard = true;
                    Some(block.label.clone())
                }
                _ => None,
            })
            .expect("the chain starts with the `Ok` arm");
        let context = MirLoanValidationContext::new(&module).unwrap();
        let route = function(&module, "route");
        let graph = FlowGraph::new(route, &context).unwrap();
        let head = route
            .blocks
            .iter()
            .position(|block| block.label == head)
            .unwrap();
        assert_eq!(graph.successors[head].len(), 1);
    }

    #[test]
    fn a_module_the_pass_cannot_read_stays_implicit() {
        // A stray block names an unknown label.
        let mut module = implicit();
        function_mut(&mut module, "pick").blocks.push(BasicBlock {
            label: "stray".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Goto("nowhere".to_string()),
        });
        insert_ownership(&mut module);
        assert_eq!(module.ownership, MirOwnership::Implicit);
        // So does one the loan context refuses.
        let mut module = implicit();
        let pick = function_mut(&mut module, "pick");
        pick.blocks[1].label = pick.blocks[0].label.clone();
        insert_ownership(&mut module);
        assert_eq!(module.ownership, MirOwnership::Implicit);
        // And one with a function past the state budget.
        let mut module = implicit();
        insert_ownership_within(&mut module, 1);
        assert_eq!(module.ownership, MirOwnership::Implicit);
    }
}
