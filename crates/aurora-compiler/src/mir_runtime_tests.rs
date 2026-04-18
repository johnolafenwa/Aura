use super::{
    bind_args, bind_builtin_args, build_range, collect_runtime_type_substitutions,
    collect_type_params_from_type, eval_ordering, evaluate_named_args, option_none, option_some,
    render_runtime_error, result_err, result_ok, run_serialized_mir, send_error_closed,
    write_stream, CancellationContext, Env, EvaluatedMirArg, MirRuntime, TaskGroupValue, TaskValue,
};
use crate::diag::{Diagnostic, Span};
use crate::integer::IntegerValue;
use crate::mir::{
    BasicBlock, Instruction, MirArg, MirClass, MirFunction, MirLocalType, MirMatchArm, MirMethod,
    MirModule, MirParam, MirSelectArm, MirSelectKind, MirTraitImpl, Operand, Rvalue, Terminator,
};
use crate::runtime_value::{ChannelValue, EnumVariantValue, InstanceValue, RangeValue, Value};
use crate::sema::Type;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

fn test_runtime() -> MirRuntime {
    MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    )
}

fn mir_arg(name: Option<&str>, value: Operand) -> MirArg {
    MirArg {
        name: name.map(str::to_string),
        value,
        writeback_place: None,
    }
}

fn run_native_entry(
    mir_ptr: *const u8,
    mir_len: usize,
    source_path_ptr: *const u8,
    source_path_len: usize,
    source_ptr: *const u8,
    source_len: usize,
) -> i32 {
    unsafe {
        crate::mir_runtime::aurora_native_run(
            mir_ptr,
            mir_len,
            source_path_ptr,
            source_path_len,
            source_ptr,
            source_len,
        )
    }
}

#[test]
fn env_place_helpers_cover_nested_reads_and_writes() {
    let mut env = Env::default();
    env.define_typed(
        "counter",
        Type::named("Counter"),
        Value::Instance(InstanceValue {
            class_name: "Counter".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(1)),
            )]),
        }),
    );

    assert_eq!(
        env.read_place("counter.value")
            .expect("nested place should read"),
        Value::Int(IntegerValue::from_signed(1))
    );
    env.write_place("counter.value", Value::Int(IntegerValue::from_signed(4)))
        .expect("nested place should write");
    assert_eq!(
        env.read_place("counter.value")
            .expect("updated nested place should read"),
        Value::Int(IntegerValue::from_signed(4))
    );
    assert_eq!(env.place_type("counter"), Some(&Type::named("Counter")));
    env.set_place_type("counter.value", Type::named("int32"));
    assert_eq!(env.place_type("counter.value"), Some(&Type::named("int32")));

    let error = env
        .read_place("counter.missing")
        .expect_err("unknown field should fail");
    assert!(error.message.contains("has no field `missing`"));

    env.define_typed(
        "count",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(9)),
    );
    let non_instance_read = env
        .read_place("count.value")
        .expect_err("non-instance nested reads should fail");
    assert!(non_instance_read
        .message
        .contains("cannot access field `value` on non-instance MIR place `count.value`"));
    let non_instance_write = env
        .write_place("count.value", Value::Int(IntegerValue::from_signed(2)))
        .expect_err("non-instance nested writes should fail");
    assert!(non_instance_write
        .message
        .contains("cannot assign nested MIR place `count.value` on non-instance value"));

    let missing_root = env
        .read_place("missing")
        .expect_err("unknown MIR places should fail");
    assert!(missing_root.message.contains("unknown MIR place `missing`"));

    let missing_root_write = env
        .write_place("missing.value", Value::Int(IntegerValue::from_signed(2)))
        .expect_err("nested writes should reject missing roots");
    assert!(missing_root_write
        .message
        .contains("unknown MIR place `missing.value`"));

    env.write_place("count", Value::Int(IntegerValue::from_signed(11)))
        .expect("root writes should succeed");
    assert_eq!(
        env.read_place("count").expect("root place should read"),
        Value::Int(IntegerValue::from_signed(11))
    );
}

#[test]
fn mir_runtime_helper_values_and_streams_cover_option_result_and_diagnostics() {
    assert_eq!(option_some(Value::Bool(true)).render(), "Option.Some(true)");
    assert_eq!(option_none().render(), "Option.None");
    assert_eq!(result_ok(Value::Bool(false)).render(), "Result.Ok(false)");
    assert_eq!(
        result_err(Value::String("oops".to_string())).render(),
        "Result.Err(oops)"
    );
    assert_eq!(
        send_error_closed(Value::Int(IntegerValue::from_signed(5))).render(),
        "SendError.Closed(5)"
    );

    let diagnostic = Diagnostic::at(Span::new(2, 3), "division by zero");
    let rendered = render_runtime_error("/tmp/test.au", "def main():\n    1 / 0\n", &diagnostic);
    assert!(rendered.contains("/tmp/test.au"));
    assert!(rendered.contains("division by zero"));

    let mut buffer = Vec::new();
    write_stream(&mut buffer, "aurora").expect("write_stream should flush successfully");
    assert_eq!(String::from_utf8(buffer).unwrap(), "aurora");
}

struct FlushFailWriter;

impl Write for FlushFailWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
    }
}

#[test]
fn mir_runtime_stream_and_entrypoint_helpers_cover_success_and_error_paths() {
    let flush_error = write_stream(&mut FlushFailWriter, "aurora")
        .expect_err("flush failures should be surfaced");
    assert_eq!(flush_error.kind(), io::ErrorKind::BrokenPipe);

    let source = "def main() -> int32:\n    return 3\n";
    let mir = crate::lower_source_to_mir(source).expect("source should lower to MIR");
    let mir_json = serde_json::to_vec(&mir).expect("MIR should serialize");
    let source_path = b"/tmp/runtime_entry.au";
    let code = run_native_entry(
        mir_json.as_ptr(),
        mir_json.len(),
        source_path.as_ptr(),
        source_path.len(),
        source.as_ptr(),
        source.len(),
    );
    assert_eq!(code, 3);

    let invalid_json = b"not-json";
    let invalid_code = run_native_entry(
        invalid_json.as_ptr(),
        invalid_json.len(),
        source_path.as_ptr(),
        source_path.len(),
        source.as_ptr(),
        source.len(),
    );
    assert_eq!(invalid_code, 1);

    let tiny = [b'x'];
    let oversized_code = run_native_entry(
        tiny.as_ptr(),
        (1 << 30) + 1,
        source_path.as_ptr(),
        source_path.len(),
        source.as_ptr(),
        source.len(),
    );
    assert_eq!(oversized_code, 1);
}

#[test]
fn mir_runtime_public_run_wrappers_cover_serialized_success_and_error_paths() {
    let source = "def main() -> int32:\n    return 7\n";
    let mir = crate::lower_source_to_mir(source).expect("source should lower to MIR");

    let output = crate::mir_runtime::run(&mir).expect("run wrapper should succeed");
    assert_eq!(output.value, Value::Int(IntegerValue::from_signed(7)));
    assert_eq!(output.stdout, "");

    let serialized = serde_json::to_vec(&mir).expect("MIR should serialize");
    let from_json = run_serialized_mir(&serialized, "/tmp/demo.au", source)
        .expect("serialized MIR should execute");
    assert_eq!(from_json.value, Value::Int(IntegerValue::from_signed(7)));

    let error = run_serialized_mir(b"{", "/tmp/demo.au", source)
        .expect_err("invalid serialized MIR should fail");
    assert!(error.message.contains("failed to deserialize embedded MIR"));
}

#[test]
fn mir_runtime_argument_binding_helpers_cover_named_and_positional_cases() {
    let mut env = Env::default();
    env.define_typed(
        "count",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(7)),
    );
    let evaluated = evaluate_named_args(
        &[
            MirArg {
                name: Some("value".to_string()),
                value: Operand::Place("count".to_string()),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Bool(true),
                writeback_place: Some("flag".to_string()),
            },
        ],
        &env,
    )
    .expect("args should evaluate");
    assert_eq!(evaluated[0].value, Value::Int(IntegerValue::from_signed(7)));
    assert_eq!(evaluated[1].writeback_place.as_deref(), Some("flag"));

    let bound = bind_builtin_args(
        &["left", "right"],
        vec![
            EvaluatedMirArg {
                name: Some("right".to_string()),
                value: Value::Int(IntegerValue::from_signed(2)),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: None,
                value: Value::Int(IntegerValue::from_signed(1)),
                writeback_place: None,
            },
        ],
    )
    .expect("args should bind");
    assert_eq!(bound[0].value, Value::Int(IntegerValue::from_signed(1)));
    assert_eq!(bound[1].name.as_deref(), Some("right"));

    let params = vec![
        MirParam {
            name: "left".to_string(),
            passing: crate::mir::MirReceiverKind::Value,
            ty: Type::named("int32"),
        },
        MirParam {
            name: "right".to_string(),
            passing: crate::mir::MirReceiverKind::Value,
            ty: Type::named("int32"),
        },
    ];
    let rebound = bind_args(&params, bound.clone()).expect("mir params should bind");
    assert_eq!(rebound.len(), 2);

    let missing = bind_builtin_args(
        &["value"],
        vec![EvaluatedMirArg {
            name: Some("other".to_string()),
            value: Value::Bool(true),
            writeback_place: None,
        }],
    )
    .err()
    .expect("unknown MIR argument should fail");
    assert!(missing.message.contains("unknown MIR argument"));

    let duplicate = bind_builtin_args(
        &["value"],
        vec![
            EvaluatedMirArg {
                name: Some("value".to_string()),
                value: Value::Int(IntegerValue::from_signed(1)),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: Some("value".to_string()),
                value: Value::Int(IntegerValue::from_signed(2)),
                writeback_place: None,
            },
        ],
    )
    .expect("duplicate named MIR arguments should keep the last value");
    assert_eq!(duplicate[0].value, Value::Int(IntegerValue::from_signed(2)));

    let too_many = bind_builtin_args(
        &["value"],
        vec![
            EvaluatedMirArg {
                name: None,
                value: Value::Bool(true),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: None,
                value: Value::Bool(false),
                writeback_place: None,
            },
        ],
    )
    .err()
    .expect("extra positional MIR arguments should fail");
    assert!(too_many.message.contains("too many MIR arguments"));

    let missing_required = bind_builtin_args(
        &["left", "right"],
        vec![EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: None,
        }],
    )
    .err()
    .expect("missing MIR arguments should fail");
    assert!(missing_required.message.contains("missing MIR argument"));

    let eval_error = evaluate_named_args(
        &[MirArg {
            name: Some("value".to_string()),
            value: Operand::Place("missing".to_string()),
            writeback_place: None,
        }],
        &env,
    )
    .err()
    .expect("reading a missing MIR place should fail");
    assert!(eval_error.message.contains("unknown MIR place `missing`"));

    let unit_value = evaluate_named_args(
        &[MirArg {
            name: Some("unit".to_string()),
            value: Operand::Unit,
            writeback_place: None,
        }],
        &env,
    )
    .expect("unit operands should evaluate");
    assert_eq!(unit_value[0].value, Value::Unit);
}

#[test]
fn mir_runtime_deadline_helper_rejects_overflowing_instants() {
    let error = super::deadline_after_millis_with(u64::MAX, |_| None)
        .expect_err("overflowing instant deadlines should be rejected");
    assert!(error
        .message
        .contains("overflows the MIR runtime deadline range"));
}

#[test]
fn mir_runtime_complexity_guard_rejects_excessive_instruction_counts() {
    let module = MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Eval {
                        value: Operand::Unit
                    };
                    super::MAX_RUNTIME_INSTRUCTIONS + 1
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };

    let error = super::validate_runtime_module_complexity(&module)
        .expect_err("oversized MIR modules should be rejected");
    assert!(error.message.contains("instruction limit"));
}

#[test]
fn mir_runtime_writeback_and_spawn_helpers_cover_borrow_mut_edges() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "target",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );

    let params = vec![
        MirParam {
            name: "source".to_string(),
            passing: crate::mir::MirReceiverKind::Borrow,
            ty: Type::named("int32"),
        },
        MirParam {
            name: "target".to_string(),
            passing: crate::mir::MirReceiverKind::BorrowMut,
            ty: Type::named("int32"),
        },
    ];
    let evaluated_args = vec![
        EvaluatedMirArg {
            name: Some("source".to_string()),
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: Some("target".to_string()),
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: Some("target".to_string()),
        },
    ];
    runtime
        .apply_borrowed_param_writebacks(
            &params,
            &evaluated_args,
            &[
                (0, Value::Int(IntegerValue::from_signed(4))),
                (1, Value::Int(IntegerValue::from_signed(7))),
                (9, Value::Int(IntegerValue::from_signed(11))),
            ],
            &mut env,
        )
        .expect("borrow-mut writebacks should update explicit writeback places");
    assert_eq!(
        env.read_place("target"),
        Ok(Value::Int(IntegerValue::from_signed(7)))
    );

    let missing_writeback = runtime
        .apply_borrowed_param_writebacks(
            &params,
            &[
                EvaluatedMirArg {
                    name: Some("source".to_string()),
                    value: Value::Int(IntegerValue::from_signed(1)),
                    writeback_place: None,
                },
                EvaluatedMirArg {
                    name: Some("target".to_string()),
                    value: Value::Int(IntegerValue::from_signed(1)),
                    writeback_place: None,
                },
            ],
            &[(1, Value::Int(IntegerValue::from_signed(9)))],
            &mut env,
        )
        .expect_err("borrow-mut writebacks require an explicit writeback place");
    assert!(missing_writeback
        .message
        .contains("requires a writeback place"));

    let by_value = MirFunction {
        name: "work".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: vec![MirParam {
            name: "value".to_string(),
            passing: crate::mir::MirReceiverKind::Value,
            ty: Type::named("int32"),
        }],
        local_types: Vec::new(),
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: Vec::new(),
    };
    runtime
        .require_spawnable_function(&by_value)
        .expect("by-value MIR functions should be spawnable");
    let spawn_error = runtime
        .require_spawnable_function(&MirFunction {
            params: vec![MirParam {
                name: "value".to_string(),
                passing: crate::mir::MirReceiverKind::Borrow,
                ty: Type::named("int32"),
            }],
            ..by_value
        })
        .expect_err("borrowed params should not be spawnable in MIR");
    assert!(spawn_error
        .message
        .contains("does not yet support borrowed parameter `value`"));
}

#[test]
fn mir_runtime_builtin_call_surface_covers_named_and_error_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed("delay", Type::named("Duration"), Value::Duration(0));
    env.define_typed(
        "negative_delay",
        Type::named("Duration"),
        Value::Duration(-1),
    );
    env.define_typed(
        "neg",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(-4)),
    );
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("12".to_string()),
    );
    env.define_typed(
        "word",
        Type::named("String"),
        Value::String("Aurora".to_string()),
    );

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("range".to_string()),
                &[MirArg {
                    name: Some("stop".to_string()),
                    value: Operand::Int(3),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("named range call should succeed"),
        Value::Range(RangeValue { start: 0, end: 3 })
    );
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("Vec".to_string()),
                &[],
                &mut env
            )
            .expect("Vec() should succeed"),
        Value::Vec(_)
    ));
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("Set".to_string()),
                &[],
                &mut env
            )
            .expect("Set() should succeed"),
        Value::Set(_)
    ));
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("Map".to_string()),
                &[],
                &mut env
            )
            .expect("Map() should succeed"),
        Value::Map(_)
    ));
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("tasks".to_string()),
                &[],
                &mut env
            )
            .expect("tasks() should succeed"),
        Value::TaskGroup(_)
    ));
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("cancelled".to_string()),
                &[],
                &mut env,
            )
            .expect("cancelled() should succeed"),
        Value::Bool(false)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("after".to_string()),
                &[MirArg {
                    name: Some("duration".to_string()),
                    value: Operand::Place("delay".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("after() should accept duration values"),
        Value::Duration(0)
    );
    let after_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("after".to_string()),
            &[mir_arg(Some("duration"), Operand::Bool(true))],
            &mut env,
        )
        .expect_err("after() should reject non-duration values");
    assert!(after_error.message.contains("expects a duration value"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("sleep".to_string()),
                &[MirArg {
                    name: Some("duration".to_string()),
                    value: Operand::Place("delay".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("sleep() should accept zero duration"),
        Value::Unit
    );
    let sleep_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sleep".to_string()),
            &[MirArg {
                name: Some("duration".to_string()),
                value: Operand::Place("negative_delay".to_string()),
                writeback_place: None,
            }],
            &mut env,
        )
        .expect_err("negative sleep durations should fail");
    assert!(sleep_error
        .message
        .contains("does not fit in the MIR runtime timer range"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("abs".to_string()),
                &[mir_arg(None, Operand::Place("neg".to_string()))],
                &mut env,
            )
            .expect("abs(int) should succeed"),
        Value::Int(IntegerValue::from_signed(4))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("min".to_string()),
                &[
                    mir_arg(None, Operand::Int(8)),
                    mir_arg(None, Operand::Int(3))
                ],
                &mut env,
            )
            .expect("min(int, int) should succeed"),
        Value::Int(IntegerValue::from_signed(3))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("max".to_string()),
                &[
                    mir_arg(None, Operand::Float(1.5)),
                    mir_arg(None, Operand::Float(2.5)),
                ],
                &mut env,
            )
            .expect("max(float, float) should succeed"),
        Value::Float(2.5)
    );
    let min_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("min".to_string()),
            &[
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Bool(true)),
            ],
            &mut env,
        )
        .expect_err("min() should reject mismatched types");
    assert!(min_error
        .message
        .contains("expects matching numeric arguments"));
    let sqrt_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sqrt".to_string()),
            &[mir_arg(None, Operand::Int(9))],
            &mut env,
        )
        .expect_err("sqrt() should reject integer operands");
    assert!(sqrt_error
        .message
        .contains("expects `float32` or `float64`"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_int32".to_string()),
                &[MirArg {
                    name: Some("text".to_string()),
                    value: Operand::Place("text".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("parse_int32() should succeed"),
        result_ok(Value::Int(IntegerValue::from_signed(12)))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_float64".to_string()),
                &[MirArg {
                    name: Some("text".to_string()),
                    value: Operand::Place("word".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("parse_float64() should return Result.Err for bad strings"),
        result_err(Value::String("invalid float literal".to_string()))
    );
    let queue_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("queue".to_string()),
            &[
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Int(2)),
            ],
            &mut env,
        )
        .expect_err("queue() should reject extra arguments");
    assert!(queue_error
        .message
        .contains("expects at most one optional `capacity` argument"));
}

#[test]
fn mir_runtime_member_call_dispatch_covers_builtin_runtime_and_trait_receivers() {
    let mut runtime = test_runtime();
    runtime.functions.insert(
        "widget_render".to_string(),
        MirFunction {
            name: "widget_render".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("String"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::String("widget".to_string())),
            }],
        },
    );
    runtime.functions.insert(
        "status_label".to_string(),
        MirFunction {
            name: "status_label".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("String"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::String("done".to_string())),
            }],
        },
    );
    runtime.classes.insert(
        "Widget".to_string(),
        MirClass {
            name: "Widget".to_string(),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: vec![MirMethod {
                name: "render".to_string(),
                function_name: "widget_render".to_string(),
                receiver: Some(crate::mir::MirReceiverKind::Borrow),
            }],
        },
    );
    runtime.trait_impls.push(MirTraitImpl {
        trait_name: "Label".to_string(),
        trait_args: Vec::new(),
        for_type: Type::named("Status"),
        methods: vec![MirMethod {
            name: "label".to_string(),
            function_name: "status_label".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
        }],
    });

    let mut env = Env::default();
    env.define_typed(
        "number",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(7)),
    );
    env.define_typed("ratio", Type::named("float64"), Value::Float(4.0));
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("Aurora".to_string()),
    );
    env.define_typed(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "counts",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define_typed(
        "seen",
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Value::Set(crate::runtime_value::SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        }),
    );
    env.define_typed(
        "jobs",
        Type::Named("Queue".to_string(), vec![Type::named("int32")]),
        Value::Channel(ChannelValue::new()),
    );
    env.define_typed(
        "task",
        Type::Named("Task".to_string(), vec![Type::named("bool")]),
        Value::Task(TaskValue::from_handle(std::thread::spawn(|| {
            Ok(Value::Bool(true))
        }))),
    );
    env.define_typed(
        "group",
        Type::named("TaskGroup"),
        Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
    );
    env.define_typed(
        "widget",
        Type::named("Widget"),
        Value::Instance(InstanceValue {
            class_name: "Widget".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Done".to_string(),
            payloads: Vec::new(),
        }),
    );
    env.define_typed("unit", Type::Unit, Value::Unit);

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("ratio".to_string()),
                    field: "sqrt".to_string(),
                    receiver_place: Some("ratio".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("float.sqrt() should succeed"),
        Value::Float(2.0)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("number".to_string()),
                    field: "to_string".to_string(),
                    receiver_place: Some("number".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("int.to_string() should succeed"),
        Value::String("7".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("flag".to_string()),
                    field: "to_string".to_string(),
                    receiver_place: Some("flag".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("bool.to_string() should succeed"),
        Value::String("true".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("text".to_string()),
                    field: "contains".to_string(),
                    receiver_place: Some("text".to_string()),
                },
                &[mir_arg(None, Operand::String("ror".to_string()))],
                &mut env,
            )
            .expect("string member calls should dispatch"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("values".to_string()),
                    field: "insert".to_string(),
                    receiver_place: Some("values".to_string()),
                },
                &[
                    mir_arg(None, Operand::Int(1)),
                    mir_arg(None, Operand::Int(9)),
                ],
                &mut env,
            )
            .expect("vec member calls should dispatch"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("counts".to_string()),
                    field: "clear".to_string(),
                    receiver_place: Some("counts".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("map member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("seen".to_string()),
                    field: "insert".to_string(),
                    receiver_place: Some("seen".to_string()),
                },
                &[mir_arg(None, Operand::String("go".to_string()))],
                &mut env,
            )
            .expect("set member calls should dispatch"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("jobs".to_string()),
                    field: "close".to_string(),
                    receiver_place: Some("jobs".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("channel member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("task".to_string()),
                    field: "result".to_string(),
                    receiver_place: Some("task".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("task member calls should dispatch"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("group".to_string()),
                    field: "cancel".to_string(),
                    receiver_place: Some("group".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("task-group member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("widget".to_string()),
                    field: "render".to_string(),
                    receiver_place: Some("widget".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("class member calls should dispatch"),
        Value::String("widget".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("status".to_string()),
                    field: "label".to_string(),
                    receiver_place: Some("status".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("runtime type fallback trait dispatch should succeed"),
        Value::String("done".to_string())
    );

    let unsupported = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("unit".to_string()),
                field: "missing".to_string(),
                receiver_place: Some("unit".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("unsupported members should fail cleanly");
    assert!(unsupported
        .message
        .contains("unsupported MIR member call `missing`"));
}

#[test]
fn mir_runtime_builtin_error_surface_covers_additional_builtin_branches() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "huge_unsigned",
        Type::named("uint128"),
        Value::Int(IntegerValue::from_literal((i128::MAX as u128) + 1)),
    );
    env.define_typed(
        "min_signed",
        Type::named("int128"),
        Value::Int(IntegerValue::from_signed(i128::MIN)),
    );
    env.define_typed(
        "word",
        Type::named("String"),
        Value::String("aurora".to_string()),
    );
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "negative_duration",
        Type::named("Duration"),
        Value::Duration(-1),
    );

    let after_overflow = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("after".to_string()),
            &[mir_arg(None, Operand::Place("huge_unsigned".to_string()))],
            &mut env,
        )
        .expect_err("after() should reject integer durations outside signed range");
    assert!(after_overflow
        .message
        .contains("must fit in signed timer range"));

    let after_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("after".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("after() should reject non-duration values");
    assert!(after_type
        .message
        .contains("expects a duration value in MIR runtime"));

    let sleep_range = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sleep".to_string()),
            &[mir_arg(
                None,
                Operand::Place("negative_duration".to_string()),
            )],
            &mut env,
        )
        .expect_err("sleep() should reject negative durations");
    assert!(sleep_range
        .message
        .contains("does not fit in the MIR runtime timer range"));

    let abs_overflow = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("abs".to_string()),
            &[mir_arg(None, Operand::Place("min_signed".to_string()))],
            &mut env,
        )
        .expect_err("abs() should reject signed overflow");
    assert!(abs_overflow
        .message
        .contains("overflowed the signed integer range"));

    let parse_int64_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("parse_int64".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("parse_int64() should reject non-strings");
    assert!(parse_int64_type
        .message
        .contains("expects `String`, found `true`"));

    let parse_float64_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("parse_float64".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("parse_float64() should reject non-strings");
    assert!(parse_float64_type
        .message
        .contains("expects `String`, found `true`"));

    let unknown = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("missing".to_string()),
            &[],
            &mut env,
        )
        .expect_err("unknown MIR functions should fail");
    assert!(unknown.message.contains("unknown MIR function `missing`"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_int32".to_string()),
                &[mir_arg(None, Operand::Place("word".to_string()))],
                &mut env,
            )
            .expect("parse_int32() should still return Result.Err for invalid strings"),
        result_err(Value::String("invalid digit found in string".to_string()))
    );
}

#[test]
fn mir_runtime_member_error_surface_covers_remaining_dispatch_branches() {
    let mut runtime = test_runtime();
    runtime.classes.insert(
        "Empty".to_string(),
        MirClass {
            name: "Empty".to_string(),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
        },
    );
    runtime.classes.insert(
        "Broken".to_string(),
        MirClass {
            name: "Broken".to_string(),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: vec![MirMethod {
                name: "render".to_string(),
                function_name: "missing_impl".to_string(),
                receiver: Some(crate::mir::MirReceiverKind::Borrow),
            }],
        },
    );
    runtime.trait_impls.push(MirTraitImpl {
        trait_name: "Render".to_string(),
        trait_args: Vec::new(),
        for_type: Type::named("Status"),
        methods: vec![MirMethod {
            name: "render".to_string(),
            function_name: "missing_trait_impl".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
        }],
    });

    let mut env = Env::default();
    env.define_typed("ratio", Type::named("float64"), Value::Float(4.0));
    env.define_typed(
        "number",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(7)),
    );
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        }),
    );
    env.define_typed(
        "ghost",
        Type::named("Ghost"),
        Value::Instance(InstanceValue {
            class_name: "Ghost".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "empty",
        Type::named("Empty"),
        Value::Instance(InstanceValue {
            class_name: "Empty".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "broken",
        Type::named("Broken"),
        Value::Instance(InstanceValue {
            class_name: "Broken".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        }),
    );

    let sqrt_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("ratio".to_string()),
                field: "sqrt".to_string(),
                receiver_place: Some("ratio".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("sqrt() should reject extra arguments");
    assert!(sqrt_args.message.contains("`sqrt` does not take arguments"));

    let int_to_string_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("number".to_string()),
                field: "to_string".to_string(),
                receiver_place: Some("number".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("int.to_string() should reject extra arguments");
    assert!(int_to_string_args
        .message
        .contains("`to_string` does not take arguments"));

    let bool_to_string_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("flag".to_string()),
                field: "to_string".to_string(),
                receiver_place: Some("flag".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("bool.to_string() should reject extra arguments");
    assert!(bool_to_string_args
        .message
        .contains("`to_string` does not take arguments"));

    let len_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "len".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("len() should reject extra arguments");
    assert!(len_args.message.contains("`len` does not take arguments"));

    let push_no_place = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "push".to_string(),
                receiver_place: None,
            },
            &[mir_arg(None, Operand::Int(9))],
            &mut env,
        )
        .expect_err("push() should require a mutable receiver place");
    assert!(push_no_place
        .message
        .contains("requires a mutable vector place"));

    let internal_index_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "__index".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[mir_arg(None, Operand::Int(0))],
            &mut env,
        )
        .expect_err("internal __index should enforce operand count");
    assert!(internal_index_args
        .message
        .contains("requires index, line, and column operands"));

    let internal_set_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "__set_index".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[
                mir_arg(None, Operand::Int(0)),
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Int(2)),
            ],
            &mut env,
        )
        .expect_err("internal __set_index should enforce operand count");
    assert!(internal_set_args
        .message
        .contains("requires index, value, line, and column operands"));

    let unsupported_vector_method = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "mystery".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("unknown vector methods should fail");
    assert!(unsupported_vector_method
        .message
        .contains("unsupported vector method `mystery`"));

    let unknown_class = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("ghost".to_string()),
                field: "render".to_string(),
                receiver_place: Some("ghost".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("unknown classes should fail");
    assert!(unknown_class.message.contains("unknown MIR class `Ghost`"));

    let missing_class_method = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("empty".to_string()),
                field: "render".to_string(),
                receiver_place: Some("empty".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("missing class methods should fail");
    assert!(missing_class_method
        .message
        .contains("class `Empty` has no MIR method `render`"));

    let missing_method_body = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("broken".to_string()),
                field: "render".to_string(),
                receiver_place: Some("broken".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("missing method bodies should fail");
    assert!(missing_method_body
        .message
        .contains("unknown MIR method body `missing_impl`"));

    let missing_trait_method_body = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("status".to_string()),
                field: "render".to_string(),
                receiver_place: Some("status".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("missing trait method bodies should fail");
    assert!(missing_trait_method_body
        .message
        .contains("unknown MIR method body `missing_trait_impl`"));
}

#[test]
fn mir_runtime_range_and_type_substitution_helpers_cover_remaining_paths() {
    let range = build_range(vec![
        EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(2)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(5)),
            writeback_place: None,
        },
    ])
    .expect("range should build");
    assert_eq!(range, Value::Range(RangeValue { start: 2, end: 5 }));
    let named_range = build_range(vec![EvaluatedMirArg {
        name: Some("stop".to_string()),
        value: Value::Int(IntegerValue::from_signed(3)),
        writeback_place: None,
    }])
    .expect("named stop should build range from zero");
    assert_eq!(named_range, Value::Range(RangeValue { start: 0, end: 3 }));
    let range_error = build_range(vec![EvaluatedMirArg {
        name: Some("unknown".to_string()),
        value: Value::Int(IntegerValue::from_signed(1)),
        writeback_place: None,
    }])
    .expect_err("unknown range argument should fail");
    assert!(range_error.message.contains("unknown MIR `range` argument"));

    let mut substitutions = HashMap::new();
    collect_runtime_type_substitutions(
        &Type::Named(
            "Map".to_string(),
            vec![
                Type::TypeParam("K".to_string()),
                Type::TypeParam("V".to_string()),
            ],
        ),
        &Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        &mut substitutions,
    );
    assert_eq!(substitutions.get("K"), Some(&Type::named("String")));
    assert_eq!(substitutions.get("V"), Some(&Type::named("int32")));

    let mut collected = BTreeSet::new();
    collect_type_params_from_type(
        &Type::Named(
            "Result".to_string(),
            vec![
                Type::TypeParam("T".to_string()),
                Type::TypeParam("E".to_string()),
            ],
        ),
        &mut collected,
    );
    assert_eq!(
        collected,
        BTreeSet::from(["E".to_string(), "T".to_string()])
    );

    assert_eq!(
        eval_ordering(
            crate::ast::BinaryOp::Less,
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
        )
        .expect("int ordering should work"),
        Value::Bool(true)
    );
    let ordering_error = eval_ordering(
        crate::ast::BinaryOp::Less,
        Value::Bool(true),
        Value::Bool(false),
    )
    .expect_err("non-numeric ordering should fail");
    assert!(ordering_error.message.contains("matching numeric operands"));
}

#[test]
fn serialized_mir_helper_reports_invalid_payloads() {
    let error = run_serialized_mir(
        b"{not-json}",
        "/tmp/test.au",
        "def main() -> int32:\n    return 0\n",
    )
    .expect_err("invalid embedded MIR should fail");
    assert!(error.message.contains("failed to deserialize embedded MIR"));
}

#[test]
fn trait_impl_lookup_and_top_level_run_helpers_cover_runtime_paths() {
    let render_method = MirMethod {
        name: "render".to_string(),
        function_name: "render_impl".to_string(),
        receiver: Some(crate::mir::MirReceiverKind::Borrow),
    };
    let runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: vec![
                MirTraitImpl {
                    trait_name: "Render".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::Named(
                        "Box".to_string(),
                        vec![Type::TypeParam("T".to_string())],
                    ),
                    methods: vec![render_method.clone()],
                },
                MirTraitImpl {
                    trait_name: "Render".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::named("Widget"),
                    methods: vec![render_method.clone()],
                },
                MirTraitImpl {
                    trait_name: "Preview".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::named("Widget"),
                    methods: vec![render_method.clone()],
                },
                MirTraitImpl {
                    trait_name: "Display".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::named("Widget"),
                    methods: vec![MirMethod {
                        name: "display".to_string(),
                        function_name: "display_impl".to_string(),
                        receiver: Some(crate::mir::MirReceiverKind::Borrow),
                    }],
                },
            ],
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        runtime
            .find_trait_impl_method(
                &Type::Named("Box".to_string(), vec![Type::named("int32")]),
                "render",
            )
            .map(|method| method.function_name.as_str()),
        Some("render_impl")
    );
    assert_eq!(
        runtime
            .find_trait_impl_method_for_class_name("Widget", "display")
            .map(|method| method.function_name.as_str()),
        Some("display_impl")
    );
    assert!(
        runtime
            .find_trait_impl_method_for_class_name("Widget", "render")
            .is_none(),
        "ambiguous class-name trait lookups should return None",
    );
    assert!(runtime
        .find_trait_impl_method(&Type::named("Missing"), "render")
        .is_none());
    assert!(runtime
        .find_trait_impl_method_for_class_name("Missing", "render")
        .is_none());

    assert_eq!(
        MirRuntime::infer_value_type(&Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        })),
        Some(Type::Named("Vec".to_string(), vec![Type::named("int32")]))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Set(crate::runtime_value::SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        })),
        Some(Type::Named("Set".to_string(), vec![Type::named("String")]))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        })),
        Some(Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")]
        ))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&option_none()),
        Some(Type::Named("Option".to_string(), vec![Type::Unit]))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&result_err(Value::String("oops".to_string()))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::Unit, Type::named("String")]
        ))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&send_error_closed(Value::Bool(true))),
        Some(Type::Named(
            "SendError".to_string(),
            vec![Type::named("bool")]
        ))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Channel(ChannelValue::new())),
        None
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Task(TaskValue::from_handle(std::thread::spawn(
            || Ok(Value::Unit)
        )))),
        None
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::TaskGroup(TaskGroupValue::new(
            &CancellationContext::default()
        ))),
        None
    );

    let top_level_module =
        crate::lower_source_to_mir("print(1)\n").expect("top-level script should lower");
    let stdout = Arc::new(Mutex::new(String::new()));
    let mut top_level_runtime = MirRuntime::new(
        top_level_module,
        stdout.clone(),
        CancellationContext::default(),
    );
    assert_eq!(
        top_level_runtime
            .run_main()
            .expect("top-level script should execute"),
        Value::Int(IntegerValue::zero())
    );
    assert_eq!(stdout.lock().unwrap().as_str(), "1\n");

    let mut missing_entrypoint_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: vec![MirClass {
                name: "Box".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
            }],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        missing_entrypoint_runtime.infer_instance_type(&InstanceValue {
            class_name: "Box".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(9)),
            )]),
        }),
        Some(Type::Named("Box".to_string(), vec![Type::named("int32")]))
    );
    let entrypoint_error = missing_entrypoint_runtime
        .run_main()
        .expect_err("missing entrypoints should fail");
    assert!(entrypoint_error
        .message
        .contains("no `main` function or top-level script statements were found"));
}

#[test]
fn mir_runtime_collection_string_and_task_helpers_cover_remaining_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "other",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(3))],
        }),
    );
    env.define_typed(
        "texts",
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("String"),
            elements: vec![
                Value::String("one".to_string()),
                Value::String("two".to_string()),
            ],
        }),
    );
    env.define_typed(
        "mapping",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define_typed(
        "mapping_other",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("next".to_string()),
                Value::Int(IntegerValue::from_signed(9)),
            )],
        }),
    );
    env.define_typed(
        "flags",
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Value::Set(crate::runtime_value::SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        }),
    );
    env.define_typed(
        "jobs",
        Type::Named("Queue".to_string(), vec![Type::named("int32")]),
        Value::Channel(ChannelValue::new()),
    );

    let vec_len = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "len",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec len should succeed");
    assert_eq!(vec_len, Value::Int(IntegerValue::from_signed(2)));

    let vec_empty = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "is_empty",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec is_empty should succeed");
    assert_eq!(vec_empty, Value::Bool(false));

    let vec_clone = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "clone",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec clone should succeed");
    match vec_clone {
        Value::Vec(vector) => assert_eq!(vector.elements.len(), 2),
        other => panic!("expected vec clone, found {other:?}"),
    }

    let vec_get = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "get",
            Some("values"),
            &[mir_arg(Some("index"), Operand::Int(0))],
            &mut env,
        )
        .expect("vec get should succeed");
    assert_eq!(
        vec_get,
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );

    let vec_contains = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "contains",
            Some("values"),
            &[mir_arg(Some("value"), Operand::Int(2))],
            &mut env,
        )
        .expect("vec contains should succeed");
    assert_eq!(vec_contains, Value::Bool(true));

    let map_len = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "len",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map len should succeed");
    assert_eq!(map_len, Value::Int(IntegerValue::from_signed(1)));

    let map_empty = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "is_empty",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map is_empty should succeed");
    assert_eq!(map_empty, Value::Bool(false));

    let map_clone = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "clone",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map clone should succeed");
    match map_clone {
        Value::Map(map) => assert_eq!(map.entries.len(), 1),
        other => panic!("expected map clone, found {other:?}"),
    }

    let map_get = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "get",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("count".to_string()))],
            &mut env,
        )
        .expect("map get should succeed");
    assert_eq!(
        map_get,
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );

    let map_values = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "values",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map values should succeed");
    match map_values {
        Value::Vec(values) => assert_eq!(values.elements.len(), 1),
        other => panic!("expected vec of values, found {other:?}"),
    }

    let map_keys = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "keys",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map keys should succeed");
    match map_keys {
        Value::Vec(keys) => assert_eq!(keys.elements.len(), 1),
        other => panic!("expected vec of keys, found {other:?}"),
    }

    let map_contains = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "contains_key",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("count".to_string()))],
            &mut env,
        )
        .expect("map contains_key should succeed");
    assert_eq!(map_contains, Value::Bool(true));

    let set_len = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "len",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect("set len should succeed");
    assert_eq!(set_len, Value::Int(IntegerValue::from_signed(1)));

    let set_empty = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "is_empty",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect("set is_empty should succeed");
    assert_eq!(set_empty, Value::Bool(false));

    let set_clone = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "clone",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect("set clone should succeed");
    match set_clone {
        Value::Set(set) => assert_eq!(set.elements.len(), 1),
        other => panic!("expected set clone, found {other:?}"),
    }

    let set_contains = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "contains",
            Some("flags"),
            &[mir_arg(Some("value"), Operand::String("ready".to_string()))],
            &mut env,
        )
        .expect("set contains should succeed");
    assert_eq!(set_contains, Value::Bool(true));

    let set_index = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "__index_option",
            Some("flags"),
            &[mir_arg(Some("index"), Operand::Int(0))],
            &mut env,
        )
        .expect("set __index_option should succeed");
    assert_eq!(set_index, option_some(Value::String("ready".to_string())));

    let string_len = runtime
        .evaluate_string_method("Aurora".to_string(), "len", &[], &mut env)
        .expect("string len should succeed");
    assert_eq!(string_len, Value::Int(IntegerValue::from_signed(6)));

    let starts_with = runtime
        .evaluate_string_method(
            "Aurora".to_string(),
            "starts_with",
            &[mir_arg(Some("text"), Operand::String("Aur".to_string()))],
            &mut env,
        )
        .expect("string starts_with should succeed");
    assert_eq!(starts_with, Value::Bool(true));

    let ends_with = runtime
        .evaluate_string_method(
            "Aurora".to_string(),
            "ends_with",
            &[mir_arg(Some("text"), Operand::String("ora".to_string()))],
            &mut env,
        )
        .expect("string ends_with should succeed");
    assert_eq!(ends_with, Value::Bool(true));

    let split = runtime
        .evaluate_string_method(
            "au-ro-ra".to_string(),
            "split",
            &[mir_arg(Some("text"), Operand::String("-".to_string()))],
            &mut env,
        )
        .expect("string split should succeed");
    match split {
        Value::Vec(parts) => assert_eq!(parts.elements.len(), 3),
        other => panic!("expected split vec, found {other:?}"),
    }

    let replace = runtime
        .evaluate_string_method(
            "Aurora".to_string(),
            "replace",
            &[
                mir_arg(Some("from"), Operand::String("Aur".to_string())),
                mir_arg(Some("to"), Operand::String("Our".to_string())),
            ],
            &mut env,
        )
        .expect("string replace should succeed");
    assert_eq!(replace, Value::String("Ourora".to_string()));

    let lower = runtime
        .evaluate_string_method("AuRoRa".to_string(), "to_lower", &[], &mut env)
        .expect("string to_lower should succeed");
    assert_eq!(lower, Value::String("aurora".to_string()));

    let upper = runtime
        .evaluate_string_method("AuRoRa".to_string(), "to_upper", &[], &mut env)
        .expect("string to_upper should succeed");
    assert_eq!(upper, Value::String("AURORA".to_string()));

    let suffix = runtime
        .evaluate_string_method(
            "prefix-value".to_string(),
            "strip_suffix",
            &[mir_arg(Some("text"), Operand::String("-value".to_string()))],
            &mut env,
        )
        .expect("string strip_suffix should succeed");
    match suffix {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "Some"),
        other => panic!("expected option result, found {other:?}"),
    }

    let trim = runtime
        .evaluate_string_method("  Aurora  ".to_string(), "trim", &[], &mut env)
        .expect("string trim should succeed");
    assert_eq!(trim, Value::String("Aurora".to_string()));

    let string_clone = runtime
        .evaluate_string_method("Aurora".to_string(), "clone", &[], &mut env)
        .expect("string clone should succeed");
    assert_eq!(string_clone, Value::String("Aurora".to_string()));

    let send = runtime
        .evaluate_channel_method(
            match env.read_place("jobs").unwrap() {
                Value::Channel(channel) => channel,
                other => panic!("expected channel, found {other:?}"),
            },
            "put",
            &[mir_arg(Some("value"), Operand::Int(5))],
            &env,
        )
        .expect("queue put should succeed");
    match send {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "Ok"),
        other => panic!("expected Result from send, found {other:?}"),
    }

    let recv = runtime
        .evaluate_channel_method(
            match env.read_place("jobs").unwrap() {
                Value::Channel(channel) => channel,
                other => panic!("expected channel, found {other:?}"),
            },
            "get",
            &[],
            &env,
        )
        .expect("queue get should succeed");
    assert_eq!(recv, option_some(Value::Int(IntegerValue::from_signed(5))));

    let close = runtime
        .evaluate_channel_method(
            match env.read_place("jobs").unwrap() {
                Value::Channel(channel) => channel,
                other => panic!("expected channel, found {other:?}"),
            },
            "close",
            &[],
            &env,
        )
        .expect("channel close should succeed");
    assert_eq!(close, Value::Unit);

    let insert = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "insert",
            Some("values"),
            &[
                mir_arg(Some("index"), Operand::Int(1)),
                mir_arg(Some("value"), Operand::Int(99)),
            ],
            &mut env,
        )
        .expect("vec insert should succeed");
    assert_eq!(insert, Value::Bool(true));

    let reverse = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "reverse",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec reverse should succeed");
    assert_eq!(reverse, Value::Unit);

    let extend = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "extend",
            Some("values"),
            &[mir_arg(Some("other"), Operand::Place("other".to_string()))],
            &mut env,
        )
        .expect("vec extend should succeed");
    assert_eq!(extend, Value::Unit);

    let clear = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "clear",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec clear should succeed");
    assert_eq!(clear, Value::Unit);

    let vec_error = runtime
        .evaluate_vec_method(
            match env.read_place("texts").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "mystery",
            Some("texts"),
            &[],
            &mut env,
        )
        .expect_err("unsupported vec method should fail");
    assert!(vec_error.message.contains("unsupported vector method"));

    let map_items = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "items",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map items should succeed");
    match map_items {
        Value::Vec(entries) => assert_eq!(entries.elements.len(), 1),
        other => panic!("expected vec, found {other:?}"),
    }

    let map_extend = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "extend",
            Some("mapping"),
            &[mir_arg(
                Some("other"),
                Operand::Place("mapping_other".to_string()),
            )],
            &mut env,
        )
        .expect("map extend should succeed");
    assert_eq!(map_extend, Value::Unit);

    let map_clear = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "clear",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map clear should succeed");
    assert_eq!(map_clear, Value::Unit);

    let map_error = runtime
        .evaluate_map_method(
            match env.read_place("mapping_other").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "extend",
            Some("mapping_other"),
            &[mir_arg(Some("other"), Operand::Int(7))],
            &mut env,
        )
        .expect_err("map extend should reject non-map values");
    assert!(map_error
        .message
        .contains("requires another `Map[K, V]` value"));

    let set_insert = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "insert",
            Some("flags"),
            &[mir_arg(Some("value"), Operand::String("go".to_string()))],
            &mut env,
        )
        .expect("set insert should succeed");
    assert_eq!(set_insert, Value::Bool(true));

    let set_remove = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "remove",
            Some("flags"),
            &[mir_arg(Some("value"), Operand::String("ready".to_string()))],
            &mut env,
        )
        .expect("set remove should succeed");
    assert_eq!(set_remove, Value::Bool(true));

    let set_error = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "unknown",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect_err("unsupported set method should fail");
    assert!(set_error.message.contains("unsupported set method"));

    let contains = runtime
        .evaluate_string_method(
            "aurora".to_string(),
            "contains",
            &[mir_arg(Some("text"), Operand::String("ror".to_string()))],
            &mut env,
        )
        .expect("string contains should succeed");
    assert_eq!(contains, Value::Bool(true));

    let join = runtime
        .evaluate_string_method(
            ", ".to_string(),
            "join",
            &[mir_arg(Some("parts"), Operand::Place("texts".to_string()))],
            &mut env,
        )
        .expect("string join should succeed");
    assert_eq!(join, Value::String("one, two".to_string()));

    let strip = runtime
        .evaluate_string_method(
            "prefix-value".to_string(),
            "strip_prefix",
            &[mir_arg(
                Some("text"),
                Operand::String("prefix-".to_string()),
            )],
            &mut env,
        )
        .expect("string strip_prefix should succeed");
    match strip {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "Some"),
        other => panic!("expected option result, found {other:?}"),
    }

    let string_error = runtime
        .evaluate_string_method(
            "aurora".to_string(),
            "contains",
            &[mir_arg(Some("text"), Operand::Bool(true))],
            &mut env,
        )
        .expect_err("string contains should reject non-string args");
    assert!(string_error.message.contains("requires a `String`"));

    let join_error = runtime
        .evaluate_string_method(
            ", ".to_string(),
            "join",
            &[mir_arg(Some("parts"), Operand::Int(1))],
            &mut env,
        )
        .expect_err("string join should reject non-vectors");
    assert!(join_error.message.contains("requires `Vec[String]`"));

    let task_clone = runtime
        .evaluate_task_method(
            TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Unit))),
            "clone",
            &[],
        )
        .expect_err("task clone should be unsupported");
    assert!(task_clone
        .message
        .contains("unsupported task method `clone`"));

    let task_join = runtime
        .evaluate_task_method(
            TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Bool(true)))),
            "result",
            &[],
        )
        .expect("task result should succeed");
    assert_eq!(task_join, Value::Bool(true));

    let task_error = runtime
        .evaluate_task_method(
            TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Unit))),
            "cancel",
            &[],
        )
        .expect_err("unsupported task method should fail");
    assert!(task_error.message.contains("unsupported task method"));

    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancel = runtime
        .evaluate_task_group_method(group.clone(), "cancel", &[], &env)
        .expect("task-group cancel should succeed");
    assert_eq!(cancel, Value::Unit);

    let no_target = runtime
        .evaluate_task_group_method(group.clone(), "start", &[], &env)
        .expect_err("task-group start should reject empty args");
    assert!(no_target.message.contains("expects a target function"));

    let bad_target = runtime
        .evaluate_task_group_method(
            group,
            "start",
            &[mir_arg(Some("target"), Operand::Int(3))],
            &env,
        )
        .expect_err("task-group start should stay in MIR lowering");
    assert!(bad_target
        .message
        .contains("should lower to MIR `Spawn` directly"));
}

#[test]
fn mir_runtime_index_helpers_cover_error_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("aurora".to_string()),
    );

    let negative = runtime
        .mir_index_from_value(Value::Int(IntegerValue::from_signed(-1)))
        .expect_err("negative indices should fail");
    assert!(negative.message.contains("cannot be negative"));

    let non_integer = runtime
        .mir_index_from_value(Value::Bool(true))
        .expect_err("non-integer indices should fail");
    assert!(non_integer
        .message
        .contains("vector indices must be integers"));

    let vec_missing_place = runtime
        .evaluate_vec_method(
            crate::runtime_value::VecValue {
                element_type: Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            },
            "clear",
            None,
            &[],
            &mut env,
        )
        .expect_err("mutable vec methods should require a receiver place");
    assert!(vec_missing_place
        .message
        .contains("requires a mutable vector place"));

    for (field, args, expected) in [
        (
            "set",
            vec![
                mir_arg(Some("index"), Operand::Int(0)),
                mir_arg(Some("value"), Operand::Int(1)),
            ],
            "`set` requires a mutable vector place",
        ),
        (
            "remove",
            vec![mir_arg(Some("index"), Operand::Int(0))],
            "`remove` requires a mutable vector place",
        ),
        (
            "swap",
            vec![
                mir_arg(Some("first"), Operand::Int(0)),
                mir_arg(Some("second"), Operand::Int(1)),
            ],
            "`swap` requires a mutable vector place",
        ),
        (
            "insert",
            vec![
                mir_arg(Some("index"), Operand::Int(0)),
                mir_arg(Some("value"), Operand::Int(1)),
            ],
            "`insert` requires a mutable vector place",
        ),
        (
            "reverse",
            Vec::new(),
            "`reverse` requires a mutable vector place",
        ),
        (
            "extend",
            vec![mir_arg(Some("other"), Operand::Bool(true))],
            "`extend` requires another `Vec[T]` value",
        ),
    ] {
        let error = runtime
            .evaluate_vec_method(
                crate::runtime_value::VecValue {
                    element_type: Type::named("int32"),
                    elements: vec![Value::Int(IntegerValue::from_signed(1))],
                },
                field,
                None,
                &args,
                &mut env,
            )
            .expect_err("vector helper edge should fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in diagnostic, got `{}`",
            error.message
        );
    }

    let internal_index_oob = runtime
        .evaluate_vec_method(
            crate::runtime_value::VecValue {
                element_type: Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            },
            "__index",
            Some("missing"),
            &[
                mir_arg(None, Operand::Int(5)),
                mir_arg(None, Operand::Int(9)),
                mir_arg(None, Operand::Int(2)),
            ],
            &mut env,
        )
        .expect_err("internal vector indexing should report out-of-bounds spans");
    assert!(internal_index_oob.message.contains("out of bounds"));
    assert_eq!(internal_index_oob.span, Some(crate::diag::Span::new(9, 2)));

    let internal_set_oob = runtime
        .evaluate_vec_method(
            crate::runtime_value::VecValue {
                element_type: Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            },
            "__set_index",
            Some("missing"),
            &[
                mir_arg(None, Operand::Int(5)),
                mir_arg(None, Operand::Int(7)),
                mir_arg(None, Operand::Int(4)),
                mir_arg(None, Operand::Int(6)),
            ],
            &mut env,
        )
        .expect_err("internal indexed assignment should report out-of-bounds spans");
    assert!(internal_set_oob.message.contains("out of bounds"));
    assert_eq!(internal_set_oob.span, Some(crate::diag::Span::new(4, 6)));

    let map_missing_place = runtime
        .evaluate_map_method(
            crate::runtime_value::MapValue {
                key_type: Type::named("String"),
                value_type: Type::named("int32"),
                entries: vec![],
            },
            "clear",
            None,
            &[],
            &mut env,
        )
        .expect_err("mutable map methods should require a receiver place");
    assert!(map_missing_place
        .message
        .contains("requires a mutable map place"));

    let set_missing_place = runtime
        .evaluate_set_method(
            crate::runtime_value::SetValue {
                element_type: Type::named("String"),
                elements: vec![],
            },
            "insert",
            None,
            &[mir_arg(Some("value"), Operand::String("go".to_string()))],
            &mut env,
        )
        .expect_err("mutable set methods should require a receiver place");
    assert!(set_missing_place
        .message
        .contains("requires a mutable set place"));

    let recv_error = match runtime.execute_select(
        &[MirSelectArm {
            binding: None,
            kind: MirSelectKind::Recv {
                channel: Operand::Place("flag".to_string()),
            },
            label: "recv".to_string(),
        }],
        &mut env,
    ) {
        Ok(_) => panic!("recv select arms should require channel values"),
        Err(error) => error,
    };
    assert!(recv_error
        .message
        .contains("MIR `select` recv arm requires a channel value"));

    let send_error = match runtime.execute_select(
        &[MirSelectArm {
            binding: None,
            kind: MirSelectKind::Send {
                channel: Operand::Place("flag".to_string()),
                value: Operand::Int(1),
            },
            label: "send".to_string(),
        }],
        &mut env,
    ) {
        Ok(_) => panic!("send select arms should require channel values"),
        Err(error) => error,
    };
    assert!(send_error
        .message
        .contains("MIR `select` send arm requires a channel value"));

    let after_error = match runtime.execute_select(
        &[MirSelectArm {
            binding: None,
            kind: MirSelectKind::After {
                duration: Operand::Place("text".to_string()),
            },
            label: "after".to_string(),
        }],
        &mut env,
    ) {
        Ok(_) => panic!("after select arms should require duration-like operands"),
        Err(error) => error,
    };
    assert!(after_error
        .message
        .contains("MIR `after(...)` expects a duration-like value"));
}

#[test]
fn mir_runtime_operator_and_task_helpers_cover_additional_branches() {
    let mut runtime = test_runtime();
    let span = Some(Span::new(4, 5));

    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::And,
                Value::Bool(true),
                Value::Bool(false),
                None,
            )
            .expect("bool and should evaluate"),
        Value::Bool(false)
    );
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Or,
                Value::Bool(false),
                Value::Bool(true),
                None,
            )
            .expect("bool or should evaluate"),
        Value::Bool(true)
    );
    let bad_and = runtime
        .eval_binary(
            crate::ast::BinaryOp::And,
            Value::Bool(true),
            Value::Int(IntegerValue::from_signed(1)),
            None,
        )
        .expect_err("non-bool logical operands should fail");
    assert!(bad_and.message.contains("must both have type `bool`"));

    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Add,
                Value::String("au".to_string()),
                Value::String("rora".to_string()),
                None,
            )
            .expect("string addition should concatenate"),
        Value::String("aurora".to_string())
    );
    let overflow = runtime
        .eval_binary(
            crate::ast::BinaryOp::Add,
            Value::Int(IntegerValue::from_literal(u128::MAX)),
            Value::Int(IntegerValue::from_signed(1)),
            span,
        )
        .expect_err("integer overflow should fail");
    assert!(overflow.message.contains("integer overflow"));
    let bad_add = runtime
        .eval_binary(
            crate::ast::BinaryOp::Add,
            Value::Bool(true),
            Value::String("x".to_string()),
            None,
        )
        .expect_err("unsupported add operands should fail");
    assert!(bad_add.message.contains("matching supported operand types"));

    let bad_sub = runtime
        .eval_binary(
            crate::ast::BinaryOp::Sub,
            Value::String("x".to_string()),
            Value::String("y".to_string()),
            None,
        )
        .expect_err("string subtraction should fail");
    assert!(bad_sub.message.contains("matching numeric operands"));

    let bad_mul = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mul,
            Value::Bool(true),
            Value::Bool(false),
            None,
        )
        .expect_err("bool multiplication should fail");
    assert!(bad_mul.message.contains("matching numeric operands"));

    let div_zero = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::Int(IntegerValue::from_signed(4)),
            Value::Int(IntegerValue::from_signed(0)),
            span,
        )
        .expect_err("division by zero should fail");
    assert!(div_zero.message.contains("division by zero"));
    let bad_div = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::String("x".to_string()),
            Value::String("y".to_string()),
            None,
        )
        .expect_err("string division should fail");
    assert!(bad_div.message.contains("matching numeric operands"));
    let float_div_zero = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::Float(7.5),
            Value::Float(0.0),
            span,
        )
        .expect_err("float division by zero should fail");
    assert!(float_div_zero.message.contains("division by zero"));

    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Mod,
                Value::Float(7.5),
                Value::Float(2.0),
                None,
            )
            .expect("float remainder should evaluate"),
        Value::Float(1.5)
    );
    let bad_mod = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mod,
            Value::Bool(true),
            Value::Bool(false),
            None,
        )
        .expect_err("bool remainder should fail");
    assert!(bad_mod.message.contains("matching numeric operands"));

    let float_mod_zero = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mod,
            Value::Float(7.5),
            Value::Float(0.0),
            span,
        )
        .expect_err("float remainder by zero should fail");
    assert!(float_mod_zero.message.contains("division by zero"));

    let task = TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Bool(true))));
    let clone_error = runtime
        .evaluate_task_method(task.clone(), "clone", &[])
        .expect_err("task clone should be unsupported");
    assert!(clone_error
        .message
        .contains("unsupported task method `clone`"));
    let join_args = runtime
        .evaluate_task_method(task.clone(), "result", &[mir_arg(None, Operand::Int(1))])
        .expect_err("result should reject arguments");
    assert!(join_args.message.contains("does not take arguments"));
    let bad_task_member = runtime
        .evaluate_task_method(task, "missing", &[])
        .expect_err("unknown task members should fail");
    assert!(bad_task_member
        .message
        .contains("unsupported task method `missing`"));

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let env = Env::default();
    assert_eq!(
        runtime
            .evaluate_task_group_method(group.clone(), "cancel", &[], &env)
            .expect("group cancel should succeed"),
        Value::Unit
    );
    let spawn_error = runtime
        .evaluate_task_group_method(group.clone(), "start", &[], &env)
        .expect_err("group start should reject empty arg lists");
    assert!(spawn_error.message.contains("expects a target function"));
    let bad_group_member = runtime
        .evaluate_task_group_method(group, "missing", &[], &env)
        .expect_err("unknown task-group members should fail");
    assert!(bad_group_member
        .message
        .contains("unsupported task-group method `missing`"));
}

#[test]
fn mir_runtime_print_tolerates_poisoned_stdout_lock() {
    let stdout = Arc::new(Mutex::new(String::new()));
    let poisoned = stdout.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoned.lock().expect("poison setup lock should succeed");
        panic!("poison stdout lock");
    })
    .join();

    let mut runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        stdout.clone(),
        CancellationContext::default(),
    );
    let mut env = Env::default();
    let value = Value::Int(IntegerValue::from_signed(3));
    env.define_typed("value", Type::named("int32"), value.clone());

    let printed = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("print".to_string()),
            &[mir_arg(Some("value"), Operand::Place("value".to_string()))],
            &mut env,
        )
        .expect("poisoned stdout should not panic");
    assert_eq!(printed, Value::Unit);
    assert_eq!(
        stdout
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_str(),
        "3\n"
    );
}

#[test]
fn mir_runtime_range_rejects_unsigned_endpoints_outside_signed_index_space() {
    let error = build_range(vec![EvaluatedMirArg {
        name: Some("stop".to_string()),
        value: Value::Int(IntegerValue::from_literal((i128::MAX as u128) + 1)),
        writeback_place: None,
    }])
    .expect_err("oversized unsigned range endpoints should fail");
    assert!(error
        .message
        .contains("must fit in signed index space in MIR runtime"));
}

#[test]
fn mir_runtime_terminator_and_cleanup_helpers_cover_branch_and_error_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    let mut loop_state = HashMap::new();
    let mut cleanup_stack = Vec::new();

    env.define_typed("cond", Type::named("bool"), Value::Bool(true));
    match runtime
        .execute_terminator(
            "entry",
            &Terminator::Branch {
                condition: Operand::Place("cond".to_string()),
                then_label: "then".to_string(),
                else_label: "else".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("bool branch should succeed")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "then"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }

    env.define_typed(
        "not_bool",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    let branch_error = match runtime.execute_terminator(
        "entry",
        &Terminator::Branch {
            condition: Operand::Place("not_bool".to_string()),
            then_label: "then".to_string(),
            else_label: "else".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("non-bool branches should fail"),
        Err(error) => error,
    };
    assert!(branch_error.message.contains("must evaluate to `bool`"));

    env.define_typed(
        "iter",
        Type::named("Range"),
        Value::Range(RangeValue { start: 0, end: 2 }),
    );
    env.define_typed(
        "item",
        Type::named("int32"),
        Value::Int(IntegerValue::zero()),
    );
    match runtime
        .execute_terminator(
            "loop",
            &Terminator::ForRange {
                binding: "item".to_string(),
                iterable: Operand::Place("iter".to_string()),
                body_label: "body".to_string(),
                exit_label: "exit".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("range loop should start")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "body"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }
    assert_eq!(
        env.read_place("item")
            .expect("loop binding should be written"),
        Value::Int(IntegerValue::zero())
    );
    let _ = runtime.execute_terminator(
        "loop",
        &Terminator::ForRange {
            binding: "item".to_string(),
            iterable: Operand::Place("iter".to_string()),
            body_label: "body".to_string(),
            exit_label: "exit".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    );
    match runtime
        .execute_terminator(
            "loop",
            &Terminator::ForRange {
                binding: "item".to_string(),
                iterable: Operand::Place("iter".to_string()),
                body_label: "body".to_string(),
                exit_label: "exit".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("range loop should exit")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "exit"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }

    env.define_typed(
        "bad_iter",
        Type::named("int32"),
        Value::Int(IntegerValue::zero()),
    );
    let range_error = match runtime.execute_terminator(
        "bad-loop",
        &Terminator::ForRange {
            binding: "item".to_string(),
            iterable: Operand::Place("bad_iter".to_string()),
            body_label: "body".to_string(),
            exit_label: "exit".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("non-range iterables should fail"),
        Err(error) => error,
    };
    assert!(range_error.message.contains("requires a `Range`"));

    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(crate::runtime_value::EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        }),
    );
    match runtime
        .execute_terminator(
            "match",
            &Terminator::Match {
                scrutinee: Operand::Place("status".to_string()),
                arms: vec![
                    MirMatchArm {
                        enum_name: None,
                        variant_name: Some("Ready".to_string()),
                        label: "ready".to_string(),
                        wildcard: false,
                    },
                    MirMatchArm {
                        enum_name: None,
                        variant_name: None,
                        label: "wild".to_string(),
                        wildcard: true,
                    },
                ],
                otherwise: "other".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("match should select a branch")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "ready"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }

    let match_error = match runtime.execute_terminator(
        "match",
        &Terminator::Match {
            scrutinee: Operand::Place("bad_iter".to_string()),
            arms: Vec::new(),
            otherwise: "other".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("non-enum matches should fail"),
        Err(error) => error,
    };
    assert!(match_error.message.contains("expected an enum value"));

    let unreachable = match runtime.execute_terminator(
        "dead",
        &Terminator::Unreachable,
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("unreachable terminators should fail"),
        Err(error) => error,
    };
    assert!(unreachable
        .message
        .contains("reached unreachable MIR block"));

    let underflow = runtime
        .pop_cleanup("resource", &mut Vec::new(), &mut env, false)
        .expect_err("missing cleanup entries should fail");
    assert!(underflow.message.contains("cleanup stack underflow"));

    let mut mismatched_stack = vec!["other".to_string()];
    let mismatch = runtime
        .pop_cleanup("resource", &mut mismatched_stack, &mut env, false)
        .expect_err("mismatched cleanup entries should fail");
    assert!(mismatch.message.contains("cleanup stack mismatch"));
}

#[test]
fn mir_runtime_entrypoint_call_and_type_helpers_cover_remaining_edges() {
    assert_eq!(
        run_native_entry(
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
        ),
        1
    );

    let source = "def main() -> int32:\n    return 0\n";
    let mir = crate::lower_source_to_mir(source).expect("source should lower");
    let mir_json = serde_json::to_vec(&mir).expect("mir should serialize");
    let bad_utf8 = [0xffu8];
    assert_eq!(
        run_native_entry(
            mir_json.as_ptr(),
            mir_json.len(),
            bad_utf8.as_ptr(),
            bad_utf8.len(),
            source.as_ptr(),
            source.len(),
        ),
        1
    );
    assert_eq!(
        run_native_entry(
            mir_json.as_ptr(),
            mir_json.len(),
            b"/tmp/test.au".as_ptr(),
            b"/tmp/test.au".len(),
            bad_utf8.as_ptr(),
            bad_utf8.len(),
        ),
        1
    );
    let tiny = [b'x'];
    assert_eq!(
        run_native_entry(
            tiny.as_ptr(),
            (1 << 30) + 1,
            b"/tmp/test.au".as_ptr(),
            b"/tmp/test.au".len(),
            source.as_ptr(),
            source.len(),
        ),
        1
    );

    let mut runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: vec![MirClass {
                name: "Pair".to_string(),
                type_params: vec!["T".to_string(), "U".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "left".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
            }],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );

    assert_eq!(
        runtime.infer_instance_type(&InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::from([(
                "left".to_string(),
                Value::Int(IntegerValue::from_signed(9)),
            )]),
        }),
        Some(Type::Named(
            "Pair".to_string(),
            vec![Type::named("int32"), Type::named("Unknown")],
        )),
    );
    assert_eq!(
        runtime.infer_instance_type(&InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::new(),
        }),
        None
    );
    assert_eq!(
        runtime.infer_runtime_value_type(&option_none()),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("Unknown")]
        )),
    );
    assert_eq!(
        runtime.infer_runtime_value_type(&result_ok(Value::Int(IntegerValue::from_signed(4)))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("Unknown")],
        )),
    );

    let mut env = Env::default();
    env.define_typed(
        "pair",
        Type::Named(
            "Pair".to_string(),
            vec![Type::named("int32"), Type::named("bool")],
        ),
        Value::Instance(InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::from([(
                "left".to_string(),
                Value::Int(IntegerValue::from_signed(1)),
            )]),
        }),
    );
    env.define_typed(
        "number",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(2)),
    );
    assert_eq!(
        runtime.resolve_place_type("pair", &env),
        Some(Type::Named(
            "Pair".to_string(),
            vec![Type::named("int32"), Type::named("bool")]
        ))
    );
    assert_eq!(runtime.resolve_place_type("pair.left", &env), None);
    assert_eq!(runtime.resolve_place_type("number.value", &env), None);
    runtime
        .validate_value_fits_type(&Value::Bool(true), &Type::named("int32"), None)
        .expect("non-integer values are ignored by integer-width validation");
    let overflow = runtime
        .validate_value_fits_type(
            &Value::Int(IntegerValue::from_signed(999)),
            &Type::named("int8"),
            None,
        )
        .expect_err("overflowing integers should fail validation");
    assert!(overflow.message.contains("does not fit in `int8`"));
    assert_eq!(
        runtime
            .coerce_value_to_type(
                Value::Int(IntegerValue::from_signed(7)),
                &Type::named("float64"),
                None
            )
            .expect("int-to-float coercion should work"),
        Value::Float(7.0)
    );

    let missing_receiver = MirFunction {
        name: "touch".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: Some(crate::mir::MirReceiverKind::Borrow),
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let receiver_error = match runtime.call_function(&missing_receiver, None, Vec::new()) {
        Ok(_) => panic!("receiver methods should require an explicit receiver"),
        Err(error) => error,
    };
    assert!(receiver_error.message.contains("missing its receiver"));

    let borrow_mut = MirFunction {
        name: "mutate".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        params: vec![MirParam {
            name: "value".to_string(),
            passing: crate::mir::MirReceiverKind::BorrowMut,
            ty: Type::named("int32"),
        }],
        local_types: vec![MirLocalType {
            name: "temp".to_string(),
            ty: Type::named("int32"),
        }],
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![
                Instruction::Assign {
                    target: "temp".to_string(),
                    value: Rvalue::Use(Operand::Int(4)),
                },
                Instruction::Eval {
                    value: Operand::Place("temp".to_string()),
                },
            ],
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let group = TaskGroupValue::new(&CancellationContext::default());
    env.define_typed(
        "group",
        Type::named("TaskGroup"),
        Value::TaskGroup(group.clone()),
    );
    let outcome = runtime
        .call_function(
            &borrow_mut,
            Some(Value::Instance(InstanceValue {
                class_name: "Pair".to_string(),
                fields: BTreeMap::from([(
                    "left".to_string(),
                    Value::Int(IntegerValue::from_signed(8)),
                )]),
            })),
            vec![EvaluatedMirArg {
                name: None,
                value: Value::Int(IntegerValue::from_signed(11)),
                writeback_place: Some("value".to_string()),
            }],
        )
        .expect("borrow-mut functions should return updated writebacks");
    assert_eq!(
        outcome.updated_receiver,
        Some(Value::Instance(InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::from([(
                "left".to_string(),
                Value::Int(IntegerValue::from_signed(8)),
            )]),
        }))
    );
    assert_eq!(
        outcome.updated_params,
        vec![(0, Value::Int(IntegerValue::from_signed(11)))]
    );
    let mut cleanup_stack = Vec::new();
    runtime
        .execute_instruction(
            &Instruction::PushCleanup {
                place: "group".to_string(),
            },
            &mut env,
            &mut cleanup_stack,
        )
        .expect("push cleanup should succeed");
    assert_eq!(cleanup_stack, vec!["group".to_string()]);
    runtime
        .execute_instruction(
            &Instruction::PopCleanup {
                place: "group".to_string(),
                cancel_before_cleanup: true,
            },
            &mut env,
            &mut cleanup_stack,
        )
        .expect("pop cleanup should run the resource cleanup path");
    assert!(cleanup_stack.is_empty());

    let bad_entry = MirFunction {
        name: "broken".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "missing".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let execute_error = runtime
        .execute_function(&bad_entry, &mut Env::default())
        .expect_err("missing block labels should fail execution");
    assert!(execute_error
        .message
        .contains("unknown MIR block `missing`"));

    let cleanup_function = MirFunction {
        name: "cleanup".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "current".to_string(),
        blocks: vec![
            BasicBlock {
                label: "inner".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::ForRange {
                    binding: "i".to_string(),
                    iterable: Operand::Place("iter".to_string()),
                    body_label: "body".to_string(),
                    exit_label: "after".to_string(),
                },
            },
            BasicBlock {
                label: "current".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Goto("after".to_string()),
            },
        ],
    };
    let mut loop_state =
        HashMap::from([("inner".to_string(), 1i128), ("current".to_string(), 2i128)]);
    MirRuntime::clear_exited_for_range_states(
        &cleanup_function,
        "current",
        "after",
        &mut loop_state,
    );
    assert!(!loop_state.contains_key("inner"));
    assert!(loop_state.contains_key("current"));
}

#[test]
fn mir_runtime_entrypoint_and_env_helpers_cover_write_stream_and_place_edges() {
    let mut sink = Vec::new();
    write_stream(&mut sink, "aurora").expect("write_stream should write into Vec sinks");
    assert_eq!(
        String::from_utf8(sink).expect("sink should be UTF-8"),
        "aurora"
    );

    let source = "def main() -> int32:\n    print(1)\n    return 7\n";
    let module = crate::lower_source_to_mir(source).expect("source should lower to MIR");
    let mir_json = serde_json::to_vec(&module).expect("MIR should serialize");
    assert_eq!(
        super::run_serialized_mir_entrypoint(&mir_json, "/tmp/entry.au", source),
        7
    );
    assert_eq!(
        super::run_serialized_mir_entrypoint(b"{not json", "/tmp/entry.au", source),
        1
    );

    let env = Env::default();
    let empty_place = env
        .read_place("")
        .expect_err("empty places should be rejected");
    assert!(empty_place.message.contains("unknown MIR place"));
    let missing_place = env
        .read_place("missing")
        .expect_err("missing places should be rejected");
    assert!(missing_place.message.contains("unknown MIR place"));

    let mut env = Env::default();
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    let non_instance = env
        .read_place("flag.value")
        .expect_err("scalar field access should fail");
    assert!(non_instance
        .message
        .contains("cannot access field `value` on non-instance MIR place `flag.value`"));

    env.define_typed(
        "pair",
        Type::named("Pair"),
        Value::Instance(InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_field = env
        .read_place("pair.value")
        .expect_err("missing fields should fail");
    assert!(missing_field
        .message
        .contains("class `Pair` has no field `value` in MIR place `pair.value`"));

    env.write_place("", Value::Unit)
        .expect("empty roots currently write through as plain locals");
    assert_eq!(
        env.read_place("")
            .expect("empty-root write should be readable"),
        Value::Unit
    );
}

#[test]
fn mir_runtime_cleanup_and_rvalue_helpers_cover_remaining_error_paths() {
    let close_fn = MirFunction {
        name: "close_managed".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let managed_class = MirClass {
        name: "Managed".to_string(),
        type_params: Vec::new(),
        fields: vec![crate::mir::MirClassField {
            name: "value".to_string(),
            ty: Type::named("int32"),
        }],
        methods: vec![MirMethod {
            name: "close".to_string(),
            function_name: "close_managed".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        }],
    };
    let worker_class = MirClass {
        name: "Worker".to_string(),
        type_params: Vec::new(),
        fields: Vec::new(),
        methods: Vec::new(),
    };
    let broken_class = MirClass {
        name: "Broken".to_string(),
        type_params: Vec::new(),
        fields: Vec::new(),
        methods: vec![MirMethod {
            name: "close".to_string(),
            function_name: "missing_body".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        }],
    };
    let mut runtime = MirRuntime::new(
        MirModule {
            functions: vec![close_fn],
            classes: vec![managed_class, worker_class, broken_class],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    let mut env = Env::default();
    env.define_typed(
        "count",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    let non_resource = runtime
        .run_cleanup_place("count", &mut env, false)
        .expect_err("non-resource cleanup targets should fail");
    assert!(non_resource.message.contains("is not a managed resource"));

    env.define_typed(
        "worker",
        Type::named("Worker"),
        Value::Instance(InstanceValue {
            class_name: "Worker".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_close = runtime
        .run_cleanup_place("worker", &mut env, false)
        .expect_err("classes without close should fail");
    assert!(missing_close
        .message
        .contains("cannot be used with MIR `with` because it has no `close` method"));

    env.define_typed(
        "broken",
        Type::named("Broken"),
        Value::Instance(InstanceValue {
            class_name: "Broken".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_body = runtime
        .run_cleanup_place("broken", &mut env, false)
        .expect_err("missing method bodies should fail");
    assert!(missing_body
        .message
        .contains("unknown MIR method body `missing_body`"));

    env.define_typed(
        "managed",
        Type::named("Managed"),
        Value::Instance(InstanceValue {
            class_name: "Managed".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(3)),
            )]),
        }),
    );
    runtime
        .run_cleanup_place("managed", &mut env, false)
        .expect("managed resources with close methods should clean up");

    let try_non_result = match runtime.evaluate_rvalue(
        &Rvalue::Try {
            value: Operand::Int(1),
        },
        &mut env,
    ) {
        Ok(_) => panic!("MIR try should require Result values"),
        Err(error) => error,
    };
    assert!(try_non_result
        .message
        .contains("MIR `try` requires a `Result` value"));

    env.define_typed(
        "broken_result",
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Result".to_string(),
            variant_name: "Ok".to_string(),
            payloads: Vec::new(),
        }),
    );
    let invalid_payload = match runtime.evaluate_rvalue(
        &Rvalue::Try {
            value: Operand::Place("broken_result".to_string()),
        },
        &mut env,
    ) {
        Ok(_) => panic!("invalid Result payloads should fail"),
        Err(error) => error,
    };
    assert!(invalid_payload
        .message
        .contains("encountered an invalid `Result` payload"));

    let non_enum_payload = match runtime.evaluate_rvalue(
        &Rvalue::VariantPayload {
            scrutinee: Operand::Int(1),
            index: 0,
        },
        &mut env,
    ) {
        Ok(_) => panic!("variant payload extraction should require enum values"),
        Err(error) => error,
    };
    assert!(non_enum_payload.message.contains("expected an enum value"));

    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        }),
    );
    let no_payload = match runtime.evaluate_rvalue(
        &Rvalue::VariantPayload {
            scrutinee: Operand::Place("status".to_string()),
            index: 0,
        },
        &mut env,
    ) {
        Ok(_) => panic!("unit variants should reject payload extraction"),
        Err(error) => error,
    };
    assert!(no_payload.message.contains("does not carry a payload"));

    let member_on_int = match runtime.evaluate_rvalue(
        &Rvalue::Member {
            object: Operand::Int(1),
            field: "value".to_string(),
        },
        &mut env,
    ) {
        Ok(_) => panic!("member access on scalars should fail"),
        Err(error) => error,
    };
    assert!(member_on_int
        .message
        .contains("cannot access field `value` on non-instance value"));

    env.define_typed(
        "empty_instance",
        Type::named("Managed"),
        Value::Instance(InstanceValue {
            class_name: "Managed".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_field = match runtime.evaluate_rvalue(
        &Rvalue::Member {
            object: Operand::Place("empty_instance".to_string()),
            field: "missing".to_string(),
        },
        &mut env,
    ) {
        Ok(_) => panic!("missing fields should fail member access"),
        Err(error) => error,
    };
    assert!(missing_field.message.contains("has no field `missing`"));
}

#[test]
fn mir_runtime_env_and_entry_helpers_cover_additional_branch_paths() {
    let mut env = Env::default();
    env.define_typed(
        "root",
        Type::named("Box"),
        Value::Instance(InstanceValue {
            class_name: "Box".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(4)),
            )]),
        }),
    );
    assert_eq!(
        env.read_place("root.value")
            .expect("nested place should read"),
        Value::Int(IntegerValue::from_signed(4))
    );
    let missing_place = env
        .write_place("missing.value", Value::Bool(true))
        .expect_err("missing MIR roots should fail");
    assert!(missing_place
        .message
        .contains("unknown MIR place `missing.value`"));
    env.write_place("", Value::Bool(true))
        .expect("empty MIR roots are currently written as plain locals");
    assert_eq!(
        env.read_place("").expect("empty root should now exist"),
        Value::Bool(true)
    );

    let runtime = test_runtime();
    assert_eq!(
        runtime
            .resolve_place_type("root", &env)
            .expect("root type should resolve"),
        Type::named("Box")
    );
    assert!(runtime.resolve_place_type("root.value", &env).is_none());
    assert!(runtime.resolve_place_type("missing", &env).is_none());

    let mut no_top_level = MirRuntime::new(
        MirModule {
            functions: vec![MirFunction {
                name: "main".to_string(),
                module_name: "<test>".to_string(),
                span: Span::new(1, 1),
                receiver: None,
                params: Vec::new(),
                local_types: Vec::new(),
                return_type: Type::named("int32"),
                entry: "entry".to_string(),
                blocks: vec![BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(9)),
                }],
            }],
            classes: vec![MirClass {
                name: "Box".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
            }],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        no_top_level.run_main().expect("main should execute"),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::ModuleNamespace(
            crate::runtime_value::ModuleNamespaceValue {
                path: "pkg.tools".to_string(),
            },
        )),
        None
    );

    let mut needs_receiver = MirRuntime::new(
        MirModule {
            functions: vec![MirFunction {
                name: "update".to_string(),
                module_name: "<test>".to_string(),
                span: Span::new(1, 1),
                receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
                params: Vec::new(),
                local_types: Vec::new(),
                return_type: Type::Unit,
                entry: "entry".to_string(),
                blocks: vec![BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Unit),
                }],
            }],
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    let update = needs_receiver
        .functions
        .get("update")
        .cloned()
        .expect("update function should exist");
    let receiver_error = match needs_receiver.call_function(&update, None, Vec::new()) {
        Ok(_) => panic!("missing MIR receivers should fail"),
        Err(error) => error,
    };
    assert!(receiver_error.message.contains("missing its receiver"));

    let panic_code = run_native_entry(
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
    );
    assert_eq!(panic_code, 1);

    let missing_main_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: Some(MirFunction {
                name: "<top-level>".to_string(),
                module_name: "<test>".to_string(),
                span: Span::new(1, 1),
                receiver: None,
                params: Vec::new(),
                local_types: Vec::new(),
                return_type: Type::named("int32"),
                entry: "entry".to_string(),
                blocks: vec![BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                }],
            }),
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        missing_main_runtime.resolve_place_type("missing", &Env::default()),
        None
    );
}
