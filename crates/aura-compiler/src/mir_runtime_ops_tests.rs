use super::{bind_runtime_op_args, resolve_runtime_ops};
use crate::mir::{CallTarget, Instruction, MirArg, MirModule, Operand, Rvalue};
use crate::runtime_ops::{ArgDefault, FunctionOp, ListOp, ResourceOp, RuntimeOp};

const BOUND_CALLS: &str = r#"
import process

def main():
    mut values: list[int64] = [1, 2, 3]
    print(values.pop())
    print(values.pop(index=0))
    events = Queue[int64]()
    indices = range(2)
    for index in indices:
        events.put(index)
    with supervisor = process.supervisor():
        match supervisor.start(name="worker", command=["/bin/true"], group=false):
            case Result.Ok(started):
                print("started")
            case Result.Err(error):
                print(error)
"#;

/// Each runtime call `main` makes, with its arguments, in order.
fn runtime_calls(module: &MirModule) -> Vec<(RuntimeOp, Vec<MirArg>)> {
    module
        .functions
        .iter()
        .filter(|function| function.name == "main")
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee: CallTarget::Runtime { op, .. },
                        args,
                        ..
                    },
                ..
            } => Some((*op, args.clone())),
            _ => None,
        })
        .collect()
}

/// How `value` prints, for comparing MIR that has no `PartialEq`.
fn shown(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}

/// How `op` binds `args`, as `shown` prints it.
fn bound(op: RuntimeOp, args: &[MirArg]) -> String {
    shown(bind_runtime_op_args(op, args))
}

/// How a binding that needs no change prints.
fn unchanged() -> String {
    shown(Ok::<Option<Vec<MirArg>>, String>(None))
}

/// How a binding to `args` prints.
fn rebound(args: Vec<MirArg>) -> String {
    shown(Ok::<_, String>(Some(args)))
}

fn positional(value: Operand) -> MirArg {
    MirArg {
        name: None,
        value,
        writeback_place: None,
    }
}

fn named(name: &str, value: Operand) -> MirArg {
    MirArg {
        name: Some(name.to_string()),
        ..positional(value)
    }
}

#[test]
fn lowered_runtime_calls_bind_every_parameter_in_order() {
    let mut module = crate::lower_source_to_mir(BOUND_CALLS).expect("source should lower");
    let calls = runtime_calls(&module);
    assert!(calls
        .iter()
        .flat_map(|(_, args)| args)
        .all(|argument| argument.name.is_none()));

    let pops: Vec<_> = calls
        .iter()
        .filter(|(op, _)| *op == RuntimeOp::List(ListOp::Pop))
        .map(|(_, args)| args.clone())
        .collect();
    assert_eq!(
        shown(&pops[0]),
        shown(vec![positional(Operand::Default(ArgDefault::Int64(-1)))])
    );
    assert_ne!(pops[1][0].value, Operand::Default(ArgDefault::Int64(-1)));

    let (_, range) = calls
        .iter()
        .find(|(op, _)| *op == RuntimeOp::Function(FunctionOp::Range))
        .expect("the fixture calls range");
    assert_eq!(range.len(), 1);

    let (_, start) = calls
        .iter()
        .find(|(op, _)| *op == RuntimeOp::Resource(ResourceOp::ProcessSupervisorStart))
        .expect("the fixture starts a process");
    let defaults: Vec<_> = start[2..10]
        .iter()
        .map(|argument| argument.value.clone())
        .collect();
    assert_eq!(
        defaults,
        vec![
            Operand::Unit,
            Operand::Default(ArgDefault::EmptyStrDict),
            Operand::Default(ArgDefault::StdioNull),
            Operand::Default(ArgDefault::StdioInherit),
            Operand::Default(ArgDefault::StdioInherit),
            Operand::Default(ArgDefault::RestartOnFailure),
            Operand::Default(ArgDefault::Milliseconds(100)),
            Operand::Default(ArgDefault::Int32(-1)),
        ]
    );
    assert_ne!(start[10].value, Operand::Bool(true));

    // Binding is idempotent, so `aura run` need not resolve what
    // `lower_implicit` resolved.
    let lowered = module.clone();
    resolve_runtime_ops(&mut module);
    assert_eq!(shown(&module), shown(&lowered));
    let program = crate::check_source(BOUND_CALLS).expect("source should check");
    let mut implicit = crate::mir::lower_implicit(&program);
    let lowered = implicit.clone();
    resolve_runtime_ops(&mut implicit);
    assert_eq!(shown(&implicit), shown(&lowered));
}

#[test]
fn hand_built_named_runtime_calls_bind_like_lowered_ones() {
    let mut module = crate::lower_source_to_mir(BOUND_CALLS).expect("source should lower");
    let lowered = module.clone();
    for function in &mut module.functions {
        for instruction in function
            .blocks
            .iter_mut()
            .flat_map(|block| &mut block.instructions)
        {
            if let Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op: RuntimeOp::List(ListOp::Pop),
                                ..
                            },
                        args,
                        ..
                    },
                ..
            } = instruction
            {
                // A hand-built call may name the argument or leave the
                // default out.
                match args[0].value {
                    Operand::Default(_) => args.clear(),
                    _ => args[0].name = Some("index".to_string()),
                }
            }
        }
    }
    assert_ne!(shown(&module), shown(&lowered));
    resolve_runtime_ops(&mut module);
    assert_eq!(shown(&module), shown(&lowered));
}

#[test]
fn runtime_op_arguments_bind_by_name_and_position() {
    let pop = RuntimeOp::List(ListOp::Pop);
    let index = || Operand::Int(0);
    assert_eq!(bound(pop, &[positional(index())]), unchanged());
    assert_eq!(
        bound(pop, &[named("index", index())]),
        rebound(vec![positional(index())])
    );
    assert_eq!(
        bound(pop, &[]),
        rebound(vec![positional(Operand::Default(ArgDefault::Int64(-1)))])
    );
    let error = |op, args: &[MirArg]| bind_runtime_op_args(op, args).expect_err("should not bind");
    assert_eq!(
        error(pop, &[named("other", index())]),
        "`pop()` has no parameter `other`"
    );
    assert_eq!(
        error(pop, &[positional(index()), positional(index())]),
        "expected `pop()` to take at most 1 argument(s), found 2"
    );
    assert_eq!(
        error(pop, &[named("index", index()), positional(index())]),
        "expected `pop()` to take at most 1 argument(s), found 2"
    );
    assert_eq!(
        error(pop, &[positional(index()), named("index", index())]),
        "`pop()` receives `index` twice"
    );
    assert_eq!(
        error(pop, &[positional(Operand::Default(ArgDefault::Int32(-1)))]),
        "`pop()` does not take that default for `index`"
    );
    assert_eq!(
        error(
            pop,
            &[named("index", Operand::Default(ArgDefault::Omitted))]
        ),
        "`pop()` does not take that default for `index`"
    );

    let clear = RuntimeOp::List(ListOp::Clear);
    assert_eq!(
        error(clear, &[positional(index())]),
        format!("expected `{}()` to take no arguments", clear.name())
    );
    let push = RuntimeOp::List(ListOp::Push);
    assert_eq!(
        error(push, &[]),
        format!("expected `{}()` to receive `value`", push.name())
    );
    assert_eq!(
        error(push, &[positional(Operand::Default(ArgDefault::Omitted))]),
        format!("`{}()` does not take that default for `value`", push.name())
    );

    let range = RuntimeOp::Function(FunctionOp::Range);
    assert_eq!(bound(range, &[positional(index())]), unchanged());
    assert_eq!(
        bound(range, &[positional(index()), positional(index())]),
        unchanged()
    );
    assert_eq!(
        bound(range, &[named("stop", index())]),
        rebound(vec![positional(index())])
    );

    let select = RuntimeOp::Function(FunctionOp::Select);
    assert_eq!(
        bound(select, &[positional(index()), positional(index())]),
        unchanged()
    );
    for args in [vec![], vec![named("source", index())]] {
        assert_eq!(
            error(select, &args),
            "expected `select(source, ...)` to receive one or more positional sources"
        );
    }
}
