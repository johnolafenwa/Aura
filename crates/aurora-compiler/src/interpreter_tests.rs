
use super::{
    cast_numeric_value, collect_type_params_from_type, option_none, option_some, render_float,
    result_err, result_ok, send_error_closed, CancellationContext, ChannelValue, EnumVariantValue,
    Env, EvalOutcome, EvaluatedArg, ExecFlow, InstanceValue, Interpreter, MapValue,
    ModuleNamespaceValue, RangeValue, SetValue, TaskGroupValue, TaskHandle, TaskState, TaskValue,
    TryRecvResult, Value, VecValue, MAX_CALL_DEPTH,
};
use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, BreakStmt, ContinueStmt, Expr, ExprKind,
    ExprStmt, ForStmt, FormatPart, FunctionDecl, IfBranch, IfStmt, LiteralPattern,
    LiteralPatternKind, MapEntryExpr, MatchArm, MatchStmt, Param, PassStmt, Pattern, ReceiverKind,
    ReturnStmt, SelectArm, Stmt, TypeRef, UnaryOp, VariantPattern, WhileStmt, WithStmt,
};
use crate::diag::Span;
use crate::integer::IntegerValue;
use crate::sema::{ModuleNamespace, TraitBound, Type};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_EXPR_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn expr(kind: ExprKind) -> Expr {
    let line = TEST_EXPR_COUNTER.fetch_add(1, Ordering::Relaxed);
    Expr {
        kind,
        span: Span::new(line, 1),
    }
}

fn positional_arg(value: Expr) -> Argument {
    Argument {
        name: None,
        span: value.span,
        value,
    }
}

fn named_arg(name: &str, value: Expr) -> Argument {
    Argument {
        name: Some(name.to_string()),
        span: value.span,
        value,
    }
}

fn type_ref(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args: Vec::new(),
        indirect: false,
        span: Span::new(1, 1),
    }
}

fn function_decl(
    name: &str,
    receiver: Option<ReceiverKind>,
    params: Vec<Param>,
    return_type: &str,
    body: Vec<Stmt>,
) -> FunctionDecl {
    FunctionDecl {
        public: true,
        name: name.to_string(),
        type_params: Vec::new(),
        type_param_bounds: BTreeMap::new(),
        receiver,
        params,
        return_type: type_ref(return_type),
        body,
        span: Span::new(1, 1),
    }
}

fn test_interpreter(source: &str) -> Interpreter {
    Interpreter {
        program: Arc::new(crate::check_source(source).expect("source should type check")),
        stdout: Arc::new(Mutex::new(String::new())),
        cancellation: CancellationContext::default(),
        module_stack: Vec::new(),
        call_depth: 0,
        expr_type_cache: RefCell::new(HashMap::new()),
    }
}

fn path_interpreter(path: &Path) -> Interpreter {
    Interpreter {
        program: Arc::new(crate::check_path(path).expect("path should type check")),
        stdout: Arc::new(Mutex::new(String::new())),
        cancellation: CancellationContext::default(),
        module_stack: Vec::new(),
        call_depth: 0,
        expr_type_cache: RefCell::new(HashMap::new()),
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("aurora-{prefix}-{}-{unique}", std::process::id()))
}

fn write_file(path: &Path, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directories should be created");
    }
    fs::write(path, source).expect("test source should be written");
}

fn expect_value_outcome(outcome: super::EvalOutcome) -> Value {
    match outcome {
        super::EvalOutcome::Value(value) => value,
        super::EvalOutcome::Return(value) => {
            panic!("expected value outcome, found return {value:?}")
        }
    }
}

fn expect_eval_error(
    result: crate::diag::Result<super::EvalOutcome>,
    message: &str,
) -> crate::diag::Diagnostic {
    match result {
        Ok(super::EvalOutcome::Value(value)) => {
            panic!("expected interpreter error for {message}, found value {value:?}")
        }
        Ok(super::EvalOutcome::Return(value)) => {
            panic!("expected interpreter error for {message}, found return {value:?}")
        }
        Err(error) => error,
    }
}

#[test]
fn render_float_preserves_whole_number_fraction() {
    assert_eq!(render_float(42.0), "42.0");
    assert_eq!(Value::Float(0.0).render(), "0.0");
}

#[test]
fn render_float_hides_float32_roundtrip_noise() {
    let float32_value = (3.14f32) as f64;
    assert_eq!(render_float(float32_value), "3.14");
    assert_eq!(Value::Float(float32_value).render(), "3.14");
}

#[test]
fn render_float_handles_nonfinite_and_full_precision_values() {
    assert_eq!(render_float(f64::INFINITY), "inf");
    let precise = std::f64::consts::PI;
    assert_eq!(render_float(precise), precise.to_string());
}

#[test]
fn value_render_and_variant_helpers_cover_runtime_surface() {
    assert_eq!(
        option_some(Value::Int(IntegerValue::from_signed(7))).render(),
        "Option.Some(7)"
    );
    assert_eq!(option_none().render(), "Option.None");
    assert_eq!(result_ok(Value::Bool(true)).render(), "Result.Ok(true)");
    assert_eq!(
        result_err(Value::String("oops".to_string())).render(),
        "Result.Err(oops)"
    );
    assert_eq!(
        send_error_closed(Value::Int(IntegerValue::from_signed(3))).render(),
        "SendError.Closed(3)"
    );
    assert_eq!(
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        })
        .render(),
        "[1, 2]"
    );
    assert_eq!(
        Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: vec![
                Value::String("a".to_string()),
                Value::String("b".to_string())
            ],
        })
        .render(),
        "Set{a, b}"
    );
    assert_eq!(
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            )],
        })
        .render(),
        "{count: 2}"
    );
    assert_eq!(
        Value::Instance(InstanceValue {
            class_name: "Point".to_string(),
            fields: BTreeMap::from([
                ("x".to_string(), Value::Float(1.5)),
                ("y".to_string(), Value::Float(2.5)),
            ]),
        })
        .render(),
        "Point(x=1.5, y=2.5)"
    );
    assert_eq!(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payload: None,
        })
        .render(),
        "Status.Ready"
    );
    assert_eq!(
        Value::Range(RangeValue { start: 1, end: 4 }).render(),
        "range(1, 4)"
    );
    assert_eq!(
        Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        })
        .render(),
        "<module pkg.tools>"
    );
}

#[test]
fn runtime_identity_and_render_helpers_cover_channels_tasks_and_groups() {
    let channel = ChannelValue::new();
    assert_eq!(Value::Channel(channel.clone()).render(), "<channel>");
    assert_eq!(channel, channel.clone());
    assert_ne!(channel, ChannelValue::new());

    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    assert_eq!(Value::Task(task.clone()).render(), "<task>");
    assert_eq!(task, task.clone());
    assert_ne!(
        task,
        TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)))
    );

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    assert_eq!(Value::TaskGroup(group.clone()).render(), "<task_group>");
    assert_eq!(group, group.clone());
    assert_ne!(group, TaskGroupValue::new(&cancellation));

    assert_eq!(Value::Duration(12).render(), "12ms");
    assert_eq!(Value::Unit.render(), "");

    assert_eq!(format!("{:?}", channel), "ChannelValue(..)");
    assert_eq!(format!("{:?}", task), "TaskValue(..)");
    assert_eq!(format!("{:?}", group), "TaskGroupValue(..)");

    assert_ne!(
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        }),
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        })
    );
    assert_ne!(
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("a".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("a".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            )],
        })
    );
}

#[test]
fn cast_numeric_value_covers_success_and_error_paths() {
    assert_eq!(
        cast_numeric_value(
            Value::Int(IntegerValue::from_signed(7)),
            &Type::named("float64"),
            Some(Span::new(1, 1)),
        )
        .expect("int to float cast should work"),
        Value::Float(7.0)
    );
    assert_eq!(
        cast_numeric_value(
            Value::Float(9.8),
            &Type::named("int32"),
            Some(Span::new(1, 1)),
        )
        .expect("float to int cast should truncate"),
        Value::Int(IntegerValue::from_signed(9))
    );
    let overflow = cast_numeric_value(
        Value::Int(IntegerValue::from_signed(999)),
        &Type::named("int8"),
        Some(Span::new(2, 3)),
    )
    .expect_err("overflowing integer cast should fail");
    assert!(overflow.message.contains("does not fit in `int8`"));
    let invalid = cast_numeric_value(
        Value::String("aurora".to_string()),
        &Type::named("int32"),
        None,
    )
    .expect_err("non-numeric cast should fail");
    assert!(invalid
        .message
        .contains("casts are only supported between numeric types"));
    let non_finite = cast_numeric_value(Value::Float(f64::INFINITY), &Type::named("int32"), None)
        .expect_err("non-finite float cast should fail");
    assert!(non_finite.message.contains("cannot cast non-finite float"));
}

#[test]
fn cast_numeric_value_reports_runtime_source_types_for_non_numeric_values() {
    let cases = [
        (Value::Bool(true), "bool"),
        (Value::String("aurora".to_string()), "String"),
        (
            Value::Vec(VecValue {
                element_type: Type::named("int32"),
                elements: Vec::new(),
            }),
            "Vec",
        ),
        (
            Value::Set(SetValue {
                element_type: Type::named("int32"),
                elements: Vec::new(),
            }),
            "Set",
        ),
        (
            Value::Map(MapValue {
                key_type: Type::named("String"),
                value_type: Type::named("int32"),
                entries: Vec::new(),
            }),
            "Map",
        ),
        (Value::Duration(5), "Duration"),
        (Value::Range(RangeValue { start: 1, end: 3 }), "Range"),
        (
            Value::ModuleNamespace(ModuleNamespaceValue {
                path: "pkg.tools".to_string(),
            }),
            "module pkg.tools",
        ),
        (Value::Unit, "None"),
        (
            Value::Instance(InstanceValue {
                class_name: "Point".to_string(),
                fields: BTreeMap::new(),
            }),
            "Point",
        ),
        (
            Value::EnumVariant(EnumVariantValue {
                enum_name: "Status".to_string(),
                variant_name: "Ready".to_string(),
                payload: None,
            }),
            "Status",
        ),
        (Value::Channel(ChannelValue::new()), "Channel"),
        (
            Value::Task(TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)))),
            "Task",
        ),
        (
            Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
            "TaskGroup",
        ),
    ];

    for (value, expected) in cases {
        let error = cast_numeric_value(value, &Type::named("int32"), None)
            .expect_err("non-numeric cast should fail");
        assert!(
            error.message.contains(expected),
            "expected cast error to mention `{expected}`, got `{}`",
            error.message
        );
    }
}

#[test]
fn interpreter_cast_and_equality_helpers_cover_additional_numeric_and_identity_edges() {
    let span = Some(Span::new(3, 4));

    let int_target_error = cast_numeric_value(
        Value::Int(IntegerValue::from_signed(7)),
        &Type::named("String"),
        span,
    )
    .expect_err("int-to-string casts should fail");
    assert!(int_target_error.message.contains("String"));

    let float_target_error = cast_numeric_value(Value::Float(1.25), &Type::named("String"), span)
        .expect_err("float-to-string casts should fail");
    assert!(float_target_error.message.contains("String"));
    let unsigned_error = cast_numeric_value(Value::Float(-1.0), &Type::named("uint8"), span)
        .expect_err("negative floats should not coerce into unsigned integers");
    assert!(unsigned_error.message.contains("does not fit in `uint8`"));

    assert_ne!(
        SetValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        },
        SetValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(2))],
        }
    );
    assert_ne!(
        MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        },
        MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            )],
        }
    );

    let channel = ChannelValue::new();
    assert_eq!(Value::Channel(channel.clone()), Value::Channel(channel));

    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    assert_eq!(Value::Task(task.clone()), Value::Task(task));

    let task_group = TaskGroupValue::new(&CancellationContext::default());
    assert_eq!(
        Value::TaskGroup(task_group.clone()),
        Value::TaskGroup(task_group)
    );
    assert_ne!(
        Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
        Value::Bool(true)
    );
}

#[test]
fn interpreter_runtime_type_helpers_cover_builtin_generic_and_runtime_value_shapes() {
    assert_eq!(
        Interpreter::lower_runtime_type(&type_ref("None")),
        Type::Unit
    );
    assert_eq!(
        Interpreter::lower_runtime_type(&type_ref("str")),
        Type::named("String")
    );
    assert_eq!(
        Interpreter::lower_runtime_type_with_type_params(&type_ref("T"), &["T".to_string()]),
        Type::TypeParam("T".to_string())
    );
    assert_eq!(
        Interpreter::lower_runtime_type(&TypeRef {
            name: "Map".to_string(),
            args: vec![type_ref("String"), type_ref("int32")],
            indirect: false,
            span: Span::new(1, 1),
        }),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")]
        )
    );

    assert_eq!(
        Interpreter::infer_value_type(&Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: Vec::new(),
        })),
        Some(Type::Named("Vec".to_string(), vec![Type::named("int32")]))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: Vec::new(),
        })),
        Some(Type::Named("Set".to_string(), vec![Type::named("String")]))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: Vec::new(),
        })),
        Some(Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")]
        ))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Duration(5)),
        Some(Type::named("Duration"))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Range(RangeValue { start: 0, end: 1 })),
        Some(Type::named("Range"))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        })),
        Some(Type::Module("pkg.tools".to_string()))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Unit),
        Some(Type::Unit)
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Instance(InstanceValue {
            class_name: "Point".to_string(),
            fields: BTreeMap::new(),
        })),
        Some(Type::named("Point"))
    );
    assert_eq!(
        Interpreter::infer_value_type(&option_some(Value::Int(IntegerValue::from_signed(7)))),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("int32")]
        ))
    );
    assert_eq!(Interpreter::infer_value_type(&option_none()), None);
    assert_eq!(
        Interpreter::infer_value_type(&result_ok(Value::Bool(true))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("bool"), Type::named("Unknown")]
        ))
    );
    assert_eq!(
        Interpreter::infer_value_type(&result_err(Value::String("oops".to_string()))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("Unknown"), Type::named("String")]
        ))
    );
    assert_eq!(
        Interpreter::infer_value_type(&send_error_closed(Value::Int(IntegerValue::from_signed(3)))),
        Some(Type::Named(
            "SendError".to_string(),
            vec![Type::named("int32")]
        ))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payload: None,
        })),
        Some(Type::named("Status"))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Channel(ChannelValue::new())),
        None
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Task(TaskValue::from_handle(thread::spawn(|| Ok(
            Value::Unit
        ))))),
        None
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::TaskGroup(TaskGroupValue::new(
            &CancellationContext::default()
        ))),
        None
    );
}

#[test]
fn channel_task_and_cancellation_helpers_cover_runtime_coordination() {
    let channel = ChannelValue::new();
    assert!(matches!(channel.try_recv(), TryRecvResult::Empty));
    channel
        .send(Value::Int(IntegerValue::from_signed(9)))
        .expect("send should succeed");
    assert_eq!(
        channel.recv_blocking(),
        Some(Value::Int(IntegerValue::from_signed(9)))
    );
    channel.close();
    assert!(matches!(channel.try_recv(), TryRecvResult::Closed));
    assert_eq!(
        channel.send(Value::Int(IntegerValue::from_signed(5))),
        Err(Value::Int(IntegerValue::from_signed(5)))
    );

    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Bool(true))));
    assert_eq!(
        task.join_result().expect("task should join"),
        Value::Bool(true)
    );
    assert_eq!(
        task.join_result().expect("completed task should be cached"),
        Value::Bool(true)
    );

    let cancellation = CancellationContext::default();
    assert!(!cancellation.is_cancelled());
    let group = TaskGroupValue::new(&cancellation);
    let child = group.child_cancellation();
    assert!(!child.is_cancelled());
    let registered = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    group.register_task(registered.clone());
    group.cancel();
    assert!(child.is_cancelled());
    assert_eq!(group.drain_tasks(), vec![registered]);
    assert!(group.drain_tasks().is_empty());

    let panicking_task =
        TaskValue::from_handle(thread::spawn(|| -> std::result::Result<Value, String> {
            panic!("boom");
        }));
    let panic_error = panicking_task
        .join_result()
        .expect_err("panicking tasks should surface a join error");
    assert!(panic_error.contains("spawned task panicked"));
}

#[test]
fn option_result_and_run_helpers_cover_remaining_runtime_shapes() {
    assert_eq!(
        option_some(Value::Int(IntegerValue::from_signed(4))),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Option".to_string(),
            variant_name: "Some".to_string(),
            payload: Some(Box::new(Value::Int(IntegerValue::from_signed(4)))),
        })
    );
    assert_eq!(
        option_none(),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Option".to_string(),
            variant_name: "None".to_string(),
            payload: None,
        })
    );
    assert_eq!(
        result_ok(Value::String("ok".to_string())),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Result".to_string(),
            variant_name: "Ok".to_string(),
            payload: Some(Box::new(Value::String("ok".to_string()))),
        })
    );
    assert_eq!(
        result_err(Value::String("err".to_string())),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Result".to_string(),
            variant_name: "Err".to_string(),
            payload: Some(Box::new(Value::String("err".to_string()))),
        })
    );
    assert_eq!(
        send_error_closed(Value::Bool(true)),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "SendError".to_string(),
            variant_name: "Closed".to_string(),
            payload: Some(Box::new(Value::Bool(true))),
        })
    );

    let run_output = crate::run_source("def main() -> int32:\n    print(41)\n    return 2\n")
        .expect("program should run successfully");
    assert_eq!(run_output.value, Value::Int(IntegerValue::from_signed(2)));
    assert_eq!(run_output.stdout, "41\n");

    let mut type_params = BTreeSet::new();
    collect_type_params_from_type(
        &Type::Named(
            "Map".to_string(),
            vec![
                Type::TypeParam("K".to_string()),
                Type::Named("Option".to_string(), vec![Type::TypeParam("V".to_string())]),
            ],
        ),
        &mut type_params,
    );
    assert_eq!(
        type_params,
        BTreeSet::from(["K".to_string(), "V".to_string()])
    );
}

#[test]
fn set_map_and_type_param_helpers_cover_equality_and_collection_utilities() {
    assert_eq!(
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(2)),
                Value::Int(IntegerValue::from_signed(1)),
            ],
        })
    );
    assert_eq!(
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![
                (
                    Value::String("a".to_string()),
                    Value::Int(IntegerValue::from_signed(1)),
                ),
                (
                    Value::String("b".to_string()),
                    Value::Int(IntegerValue::from_signed(2)),
                ),
            ],
        }),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![
                (
                    Value::String("b".to_string()),
                    Value::Int(IntegerValue::from_signed(2)),
                ),
                (
                    Value::String("a".to_string()),
                    Value::Int(IntegerValue::from_signed(1)),
                ),
            ],
        })
    );

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

    let channel = ChannelValue::new();
    let other_channel = ChannelValue::new();
    assert_eq!(channel, channel.clone());
    assert_ne!(channel, other_channel);
    assert_eq!(format!("{:?}", channel), "ChannelValue(..)");

    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    let other_task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    assert_eq!(task, task.clone());
    assert_ne!(task, other_task);
    assert_eq!(format!("{:?}", task), "TaskValue(..)");

    let group = TaskGroupValue::new(&CancellationContext::default());
    let other_group = TaskGroupValue::new(&CancellationContext::default());
    assert_eq!(group, group.clone());
    assert_ne!(group, other_group);
    assert_eq!(format!("{:?}", group), "TaskGroupValue(..)");

    for (value, expected_type) in [
        (
            Value::Vec(VecValue {
                element_type: Type::named("int32"),
                elements: vec![],
            }),
            "Vec",
        ),
        (
            Value::Set(SetValue {
                element_type: Type::named("int32"),
                elements: vec![],
            }),
            "Set",
        ),
        (
            Value::Map(MapValue {
                key_type: Type::named("String"),
                value_type: Type::named("int32"),
                entries: vec![],
            }),
            "Map",
        ),
        (Value::Duration(5), "Duration"),
        (Value::Range(RangeValue { start: 0, end: 1 }), "Range"),
        (
            Value::ModuleNamespace(ModuleNamespaceValue {
                path: "pkg.tools".to_string(),
            }),
            "module pkg.tools",
        ),
        (Value::Unit, "None"),
        (
            Value::Instance(InstanceValue {
                class_name: "Counter".to_string(),
                fields: BTreeMap::new(),
            }),
            "Counter",
        ),
        (
            Value::EnumVariant(EnumVariantValue {
                enum_name: "Status".to_string(),
                variant_name: "Ready".to_string(),
                payload: None,
            }),
            "Status",
        ),
        (Value::Channel(ChannelValue::new()), "Channel"),
        (
            Value::Task(TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)))),
            "Task",
        ),
        (
            Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
            "TaskGroup",
        ),
    ] {
        let error = cast_numeric_value(value, &Type::named("float64"), None)
            .expect_err("non-numeric values should fail numeric casts");
        assert!(
            error.message.contains(expected_type),
            "expected cast error to mention `{expected_type}`, found `{}`",
            error.message
        );
    }
}

#[test]
fn env_and_namespace_helpers_cover_scope_and_lookup_behaviour() {
    let mut env = Env::with_root();
    env.define(
        "count".to_string(),
        Value::Int(IntegerValue::from_signed(1)),
    );
    assert_eq!(
        env.get("count"),
        Some(&Value::Int(IntegerValue::from_signed(1)))
    );
    env.push_scope();
    env.define_typed(
        "name".to_string(),
        Type::named("String"),
        Value::String("aurora".to_string()),
    );
    assert_eq!(env.get_type("name"), Some(&Type::named("String")));
    env.set(
        "count".to_string(),
        Value::Int(IntegerValue::from_signed(2)),
    );
    assert_eq!(
        env.get("count"),
        Some(&Value::Int(IntegerValue::from_signed(2)))
    );
    env.pop_scope();
    assert_eq!(env.get("name"), None);

    let nested = ModuleNamespace {
        name: "tools".to_string(),
        path: "pkg.tools".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    let root = ModuleNamespace {
        name: "pkg".to_string(),
        path: "pkg".to_string(),
        source_path: None,
        modules: BTreeMap::from([("tools".to_string(), nested.clone())]),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    assert_eq!(
        Interpreter::find_namespace_in_modules(
            &BTreeMap::from([("pkg".to_string(), root)]),
            "pkg.tools",
        )
        .map(|namespace| namespace.path.clone()),
        Some("pkg.tools".to_string())
    );
}

#[test]
fn interpreter_module_resolution_helpers_cover_nested_and_ambiguous_import_paths() {
    let temp_root = unique_temp_dir("interpreter-module-resolution");
    let main_path = temp_root.join("main.au");
    write_file(
        &main_path,
        "import helpers.math\nimport helpers.other\n\ndef main() -> int32:\n    return 0\n",
    );
    write_file(
            &temp_root.join("helpers/math.au"),
            "public def work() -> int32:\n    return 7\n\npublic class Widget:\n    value: int32\n\npublic enum Status:\n    Ready\n",
        );
    write_file(
        &temp_root.join("helpers/other.au"),
        "public class Widget:\n    label: String\n\npublic enum Status:\n    Waiting\n",
    );

    let mut interpreter = path_interpreter(&main_path);
    let helpers_expr = expr(ExprKind::Name("helpers".to_string()));
    let math_expr = expr(ExprKind::Member {
        object: Box::new(helpers_expr.clone()),
        field: "math".to_string(),
    });
    let specialized_math = expr(ExprKind::Specialize {
        expr: Box::new(math_expr.clone()),
        type_args: vec![type_ref("int32")],
    });
    let widget_expr = expr(ExprKind::Member {
        object: Box::new(math_expr.clone()),
        field: "Widget".to_string(),
    });

    assert_eq!(interpreter.current_module_name(), "main");
    assert!(interpreter.current_module_namespace().is_none());
    assert_eq!(
        interpreter
            .module_namespace("helpers.math")
            .map(|namespace| namespace.path.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        interpreter.infer_module_path(&helpers_expr),
        Some("helpers".to_string())
    );
    assert_eq!(
        interpreter.infer_module_path(&math_expr),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        interpreter.infer_module_path(&specialized_math),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        interpreter.infer_module_path(&expr(ExprKind::Group(Box::new(math_expr.clone())))),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        interpreter.qualified_module_item(&widget_expr),
        Some(("helpers.math".to_string(), "Widget".to_string()))
    );
    assert!(interpreter.resolve_class_info("Widget").is_none());
    assert!(interpreter.resolve_enum_info("Status").is_none());
    assert_eq!(
        interpreter
            .resolve_class_info("helpers.math.Widget")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        interpreter
            .resolve_enum_info("helpers.math.Status")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );

    interpreter.module_stack.push("helpers.math".to_string());
    assert_eq!(interpreter.current_module_name(), "helpers.math");
    assert_eq!(
        interpreter
            .current_module_namespace()
            .map(|namespace| namespace.path.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        interpreter
            .resolve_function_info("work")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        interpreter
            .resolve_class_info("Widget")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        interpreter
            .resolve_enum_info("Status")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn interpreter_module_seed_helpers_cover_imported_registry_paths() {
    let mut interpreter = test_interpreter("def main() -> int32:\n    return 0\n");
    let nested = ModuleNamespace {
        name: "math".to_string(),
        path: "helpers.math".to_string(),
        source_path: Some("/tmp/helpers/math.au".to_string()),
        modules: BTreeMap::new(),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::from([(
            "Widget".to_string(),
            crate::sema::ClassInfo {
                module_name: "helpers.math".to_string(),
                decl: crate::ast::ClassDecl {
                    public: true,
                    copy: false,
                    name: "Widget".to_string(),
                    type_params: Vec::new(),
                    type_param_bounds: BTreeMap::new(),
                    fields: Vec::new(),
                    methods: Vec::new(),
                    span: Span::new(1, 1),
                },
                type_param_bounds: BTreeMap::new(),
                fields: BTreeMap::new(),
                methods: BTreeMap::new(),
            },
        )]),
        all_enums: BTreeMap::from([(
            "Status".to_string(),
            crate::sema::EnumInfo {
                module_name: "helpers.math".to_string(),
                decl: crate::ast::EnumDecl {
                    public: true,
                    name: "Status".to_string(),
                    type_params: Vec::new(),
                    type_param_bounds: BTreeMap::new(),
                    variants: Vec::new(),
                    span: Span::new(1, 1),
                },
                type_param_bounds: BTreeMap::new(),
                variants: BTreeMap::new(),
            },
        )]),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    let root = ModuleNamespace {
        name: "helpers".to_string(),
        path: "helpers".to_string(),
        source_path: Some("/tmp/helpers.au".to_string()),
        modules: BTreeMap::from([("math".to_string(), nested.clone())]),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::from([("math".to_string(), nested.clone())]),
    };

    let program = Arc::get_mut(&mut interpreter.program)
        .expect("interpreter program should be uniquely owned in tests");
    program.imported_modules = BTreeMap::from([("helpers".to_string(), root.clone())]);
    program.module_registry = BTreeMap::from([
        ("helpers".to_string(), root),
        ("helpers.math".to_string(), nested),
    ]);

    let mut env = Env::with_root();
    interpreter.seed_imported_modules(&mut env);
    assert_eq!(
        env.get("helpers"),
        Some(&Value::ModuleNamespace(ModuleNamespaceValue {
            path: "helpers".to_string(),
        }))
    );
    assert_eq!(
        env.get_type("helpers"),
        Some(&Type::Module("helpers".to_string()))
    );

    let helpers_expr = expr(ExprKind::Name("helpers".to_string()));
    let math_expr = expr(ExprKind::Member {
        object: Box::new(helpers_expr.clone()),
        field: "math".to_string(),
    });
    assert_eq!(
        interpreter.infer_module_path(&math_expr),
        Some("helpers.math".to_string())
    );
    assert_eq!(
        interpreter.qualified_module_item(&expr(ExprKind::Member {
            object: Box::new(math_expr.clone()),
            field: "Widget".to_string(),
        })),
        Some(("helpers.math".to_string(), "Widget".to_string()))
    );
    assert_eq!(
        interpreter
            .resolve_class_info("helpers.math.Widget")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert_eq!(
        interpreter
            .resolve_enum_info("helpers.math.Status")
            .map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
}

#[test]
fn interpreter_recursive_lookup_helpers_cover_imported_module_ambiguity() {
    let widget = crate::sema::ClassInfo {
        module_name: "helpers.math".to_string(),
        decl: crate::ast::ClassDecl {
            public: true,
            copy: false,
            name: "Widget".to_string(),
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            span: Span::new(1, 1),
        },
        type_param_bounds: BTreeMap::new(),
        fields: BTreeMap::new(),
        methods: BTreeMap::new(),
    };
    let status = crate::sema::EnumInfo {
        module_name: "helpers.math".to_string(),
        decl: crate::ast::EnumDecl {
            public: true,
            name: "Status".to_string(),
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            variants: Vec::new(),
            span: Span::new(1, 1),
        },
        type_param_bounds: BTreeMap::new(),
        variants: BTreeMap::new(),
    };
    let child = ModuleNamespace {
        name: "math".to_string(),
        path: "helpers.math".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::from([("Widget".to_string(), widget.clone())]),
        all_enums: BTreeMap::from([("Status".to_string(), status.clone())]),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    let root = ModuleNamespace {
        name: "helpers".to_string(),
        path: "helpers".to_string(),
        source_path: None,
        modules: BTreeMap::from([("math".to_string(), child.clone())]),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };

    let mut found_class = None;
    let mut ambiguous_class = false;
    let root_modules = BTreeMap::from([("helpers".to_string(), root.clone())]);
    Interpreter::find_class_in_modules(
        &root_modules,
        "Widget",
        &mut found_class,
        &mut ambiguous_class,
    );
    assert_eq!(
        found_class.map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert!(!ambiguous_class);

    let mut found_enum = None;
    let mut ambiguous_enum = false;
    let enum_modules = BTreeMap::from([("helpers".to_string(), root)]);
    Interpreter::find_enum_in_modules(
        &enum_modules,
        "Status",
        &mut found_enum,
        &mut ambiguous_enum,
    );
    assert_eq!(
        found_enum.map(|info| info.module_name.as_str()),
        Some("helpers.math")
    );
    assert!(!ambiguous_enum);

    let duplicate_child = ModuleNamespace {
        name: "other".to_string(),
        path: "helpers.other".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::from([("Widget".to_string(), widget)]),
        all_enums: BTreeMap::from([("Status".to_string(), status)]),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    let duplicate_root = ModuleNamespace {
        name: "helpers".to_string(),
        path: "helpers".to_string(),
        source_path: None,
        modules: BTreeMap::from([
            ("math".to_string(), child),
            ("other".to_string(), duplicate_child),
        ]),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    let mut ambiguous_class = false;
    Interpreter::find_class_in_modules(
        &BTreeMap::from([("helpers".to_string(), duplicate_root.clone())]),
        "Widget",
        &mut None,
        &mut ambiguous_class,
    );
    assert!(ambiguous_class);
    let mut ambiguous_enum = false;
    Interpreter::find_enum_in_modules(
        &BTreeMap::from([("helpers".to_string(), duplicate_root)]),
        "Status",
        &mut None,
        &mut ambiguous_enum,
    );
    assert!(ambiguous_enum);
}

#[test]
fn interpreter_runtime_setup_and_validation_helpers_cover_script_and_depth_paths() {
    let temp_root = unique_temp_dir("interpreter-top-level-script");
    let script_path = temp_root.join("script.au");
    write_file(
        &script_path,
        "import helpers.math\nprint(helpers.math.answer())\n",
    );
    write_file(
        &temp_root.join("helpers/math.au"),
        "public def answer() -> int32:\n    return 7\n",
    );

    let mut interpreter = path_interpreter(&script_path);
    let value = interpreter.run_main().expect("top-level script should run");
    assert_eq!(value, Value::Int(IntegerValue::zero()));
    assert_eq!(
        interpreter.stdout.lock().expect("stdout lock").as_str(),
        "7\n"
    );

    let span = Span::new(2, 3);
    let overflow = interpreter
        .validate_value_fits_type(
            &Value::Int(IntegerValue::from_signed(999)),
            &Type::named("int8"),
            span,
        )
        .expect_err("overflowing narrow integer should fail");
    assert!(overflow.message.contains("does not fit in `int8`"));
    interpreter
        .validate_value_fits_type(
            &Value::String("aurora".to_string()),
            &Type::named("int8"),
            span,
        )
        .expect("non-integer values should bypass integer width validation");

    let mut substitutions = HashMap::new();
    Interpreter::collect_runtime_type_substitutions(
        &Type::Named(
            "Result".to_string(),
            vec![Type::TypeParam("T".to_string()), Type::named("String")],
        ),
        &Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        &mut substitutions,
    );
    assert_eq!(substitutions.get("T"), Some(&Type::named("int32")));

    let mut depth_checked = test_interpreter("def helper() -> int32:\n    return 1\n");
    let helper = depth_checked
        .program
        .functions
        .get("helper")
        .expect("helper function should exist")
        .decl
        .clone();
    let module_name = depth_checked.program.module_name.clone();
    depth_checked.call_depth = super::MAX_CALL_DEPTH;
    let depth_error = match depth_checked.call_function(&helper, module_name.as_str(), Vec::new()) {
        Ok(_) => panic!("max call depth should be enforced"),
        Err(error) => error,
    };
    assert!(depth_error.message.contains("maximum call depth"));

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn interpreter_coercion_helpers_cover_numeric_and_passthrough_paths() {
    let interpreter = test_interpreter("def main():\n    pass\n");
    let span = Span::new(1, 1);

    assert_eq!(
        interpreter
            .coerce_value_to_type(
                Value::Int(IntegerValue::from_signed(7)),
                &Type::named("float64"),
                span,
            )
            .expect("int to float coercion should work"),
        Value::Float(7.0)
    );
    assert_eq!(
        interpreter
            .coerce_value_to_type(Value::Float(9.8), &Type::named("int32"), span)
            .expect("float to int coercion should work"),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        interpreter
            .coerce_value_to_type(
                Value::Int(IntegerValue::from_signed(3)),
                &Type::named("int32"),
                span,
            )
            .expect("already compatible values should pass through"),
        Value::Int(IntegerValue::from_signed(3))
    );
    assert_eq!(
        interpreter
            .coerce_value_to_type(
                Value::String("aurora".to_string()),
                &Type::named("String"),
                span,
            )
            .expect("matching runtime-backed values should pass through"),
        Value::String("aurora".to_string())
    );

    let overflow = interpreter
        .coerce_value_to_type(
            Value::Int(IntegerValue::from_signed(999)),
            &Type::named("int8"),
            span,
        )
        .expect_err("narrow integer coercion should still validate bounds");
    assert!(overflow.message.contains("does not fit in `int8`"));
}

#[test]
fn interpreter_operator_trait_helpers_cover_trait_dispatch_and_fallbacks() {
    let mut interpreter =
        test_interpreter(include_str!("../../../examples/traits/operator_traits.au"));
    let span = Span::new(1, 1);
    let left = Value::Instance(InstanceValue {
        class_name: "Point".to_string(),
        fields: BTreeMap::from([
            ("x".to_string(), Value::Int(IntegerValue::from_signed(2))),
            ("y".to_string(), Value::Int(IntegerValue::from_signed(3))),
        ]),
    });
    let right = Value::Instance(InstanceValue {
        class_name: "Point".to_string(),
        fields: BTreeMap::from([
            ("x".to_string(), Value::Int(IntegerValue::from_signed(4))),
            ("y".to_string(), Value::Int(IntegerValue::from_signed(5))),
        ]),
    });

    let added = interpreter
        .eval_binary_operator_via_trait(span, BinaryOp::Add, left.clone(), right)
        .expect("operator trait dispatch should succeed")
        .expect("Add impl should return a value");
    match added {
        Value::Instance(instance) => {
            assert_eq!(
                instance.fields.get("x"),
                Some(&Value::Int(IntegerValue::from_signed(6)))
            );
            assert_eq!(
                instance.fields.get("y"),
                Some(&Value::Int(IntegerValue::from_signed(8)))
            );
        }
        other => panic!("expected Point instance from add trait, found {:?}", other),
    }

    let negated = interpreter
        .eval_unary_operator_via_trait(span, UnaryOp::Neg, left)
        .expect("unary neg trait dispatch should succeed");
    match negated {
        Value::Instance(instance) => {
            assert_eq!(
                instance.fields.get("x"),
                Some(&Value::Int(IntegerValue::from_signed(-2)))
            );
            assert_eq!(
                instance.fields.get("y"),
                Some(&Value::Int(IntegerValue::from_signed(-3)))
            );
        }
        other => panic!("expected Point instance from neg trait, found {:?}", other),
    }

    assert_eq!(
        interpreter
            .eval_binary_operator_via_trait(
                span,
                BinaryOp::Eq,
                Value::Bool(true),
                Value::Bool(false),
            )
            .expect("non-trait operators should return None"),
        None
    );
    let unary_error = interpreter
        .eval_unary_operator_via_trait(span, UnaryOp::Not, Value::Int(IntegerValue::from_signed(1)))
        .expect_err("missing unary trait should report a normal Aurora diagnostic");
    assert!(unary_error.message.contains("`not` expects `bool`"));
    let neg_error = interpreter
        .eval_unary_operator_via_trait(span, UnaryOp::Neg, Value::String("oops".to_string()))
        .expect_err("missing neg trait should report the numeric fallback diagnostic");
    assert!(neg_error
        .message
        .contains("unary `-` expects a numeric value"));

    let method = interpreter
        .find_trait_impl_method_for_class_name("Point", "neg")
        .expect("Point neg trait method should resolve");
    assert_eq!(method.decl.name, "neg");
    assert!(interpreter.trait_impls_in_scope().count() >= 2);
}

#[test]
fn interpreter_trait_bound_resolution_helpers_cover_generic_bound_paths() {
    let interpreter = test_interpreter(
            "trait Show:\n    def show(self) -> String\n\ntrait Mapper[T]:\n    def map(self, value: T) -> T\n\nclass Name:\n    value: String\n\nclass Box[T]:\n    value: T\n\nimpl Show for Name:\n    def show(self) -> String:\n        return self.value\n\nimpl[T: Show] Mapper[T] for Box[T]:\n    def map(self, value: T) -> T:\n        return value\n\ndef main():\n    pass\n",
        );
    let trait_impl = interpreter
        .program
        .trait_impls
        .iter()
        .find(|info| info.trait_name == "Mapper")
        .expect("generic mapper impl should be present");

    let mapper_name_bound = crate::sema::TraitBound {
        trait_name: "Mapper".to_string(),
        trait_args: vec![Type::named("Name")],
    };
    let boxed_name = Type::Named("Box".to_string(), vec![Type::named("Name")]);
    let substitutions = interpreter
        .trait_impl_substitutions_for_bound(trait_impl, &boxed_name, &mapper_name_bound)
        .expect("generic impl should satisfy bound for Box[Name]");
    assert_eq!(substitutions.get("T"), Some(&Type::named("Name")));
    assert!(interpreter.type_implements_trait_bound(&boxed_name, &mapper_name_bound));

    let mapper_int_bound = crate::sema::TraitBound {
        trait_name: "Mapper".to_string(),
        trait_args: vec![Type::named("int32")],
    };
    let boxed_int = Type::Named("Box".to_string(), vec![Type::named("int32")]);
    assert!(
        interpreter
            .trait_impl_substitutions_for_bound(trait_impl, &boxed_int, &mapper_int_bound)
            .is_none(),
        "impl bound should reject type arguments that do not satisfy Show",
    );
    assert!(!interpreter.type_implements_trait_bound(&boxed_int, &mapper_int_bound));
}

#[test]
fn interpreter_trait_match_and_call_helpers_cover_additional_edges() {
    let mut interpreter = test_interpreter(
            "trait Show:\n    def show(self) -> String\n\ntrait Mapper[T]:\n    def map(self, value: T) -> T\n\nclass Name:\n    value: String\n\nclass Box[T]:\n    value: T\n\nenum Status:\n    Ready\n    Done(int32)\n\nimpl Show for Name:\n    def show(self) -> String:\n        return self.value\n\nimpl[T: Show] Mapper[T] for Box[T]:\n    def map(self, value: T) -> T:\n        return value\n\ndef work(value: int32) -> int32:\n    return value\n\ndef main():\n    pass\n",
        );
    let trait_impl = interpreter
        .program
        .trait_impls
        .iter()
        .find(|info| info.trait_name == "Mapper")
        .expect("generic mapper impl should exist");
    let boxed_name = Type::Named("Box".to_string(), vec![Type::named("Name")]);
    let boxed_int = Type::Named("Box".to_string(), vec![Type::named("int32")]);
    let substitutions = interpreter
        .trait_impl_substitutions(trait_impl, &boxed_name)
        .expect("generic impl should substitute over Box[Name]");
    assert_eq!(substitutions.get("T"), Some(&Type::named("Name")));
    assert!(
        interpreter
            .trait_impl_substitutions(trait_impl, &boxed_int)
            .is_none(),
        "generic impl bounds should reject Box[int32]",
    );

    let show_bound = TraitBound {
        trait_name: "Show".to_string(),
        trait_args: Vec::new(),
    };
    assert!(interpreter.type_implements_trait_bound(&Type::named("Name"), &show_bound));
    assert!(!interpreter.type_implements_trait_bound(&Type::named("int32"), &show_bound));

    let (method, method_substitutions) = interpreter
        .find_trait_impl_method(&boxed_name, "map")
        .expect("generic trait impl methods should resolve");
    assert_eq!(method.decl.name, "map");
    assert_eq!(method_substitutions.get("T"), Some(&Type::named("Name")));
    assert!(
        interpreter
            .find_trait_impl_method(&Type::named("Name"), "map")
            .is_none(),
        "non-matching receiver types should not resolve impl methods",
    );
    let module_name = interpreter.program.module_name.clone();

    let missing_receiver = function_decl(
        "touch",
        Some(ReceiverKind::Borrow),
        Vec::new(),
        "None",
        vec![Stmt::Pass(PassStmt {
            span: Span::new(1, 1),
        })],
    );
    let missing_receiver_error =
        match interpreter.call_function(&missing_receiver, module_name.as_str(), Vec::new()) {
            Ok(_) => panic!("methods require an explicit receiver argument"),
            Err(error) => error,
        };
    assert!(missing_receiver_error
        .message
        .contains("missing its receiver"));

    let helper = function_decl("helper", None, Vec::new(), "None", Vec::new());
    interpreter.call_depth = MAX_CALL_DEPTH;
    let call_depth_error =
        match interpreter.call_function(&helper, module_name.as_str(), Vec::new()) {
            Ok(_) => panic!("call depth guard should trip"),
            Err(error) => error,
        };
    assert!(call_depth_error.message.contains("maximum call depth"));
    interpreter.call_depth = 0;

    let span = Span::new(1, 1);
    assert_eq!(
        interpreter
            .match_pattern(
                &Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Int(IntegerValue::from_signed(1)),
                    span,
                }),
                &Value::Int(IntegerValue::from_signed(1)),
            )
            .expect("literal matches should succeed"),
        Some(None)
    );
    assert_eq!(
        interpreter
            .match_pattern(
                &Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Bool(true),
                    span,
                }),
                &Value::Bool(false),
            )
            .expect("bool literal mismatches should not error"),
        None
    );
    assert_eq!(
        interpreter
            .match_pattern(
                &Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::String("aurora".to_string()),
                    span,
                }),
                &Value::String("aurora".to_string()),
            )
            .expect("string literal matches should succeed"),
        Some(None)
    );

    let ready_pattern = Pattern::Variant(VariantPattern {
        enum_name: Some("Status".to_string()),
        variant_name: "Ready".to_string(),
        binding: None,
        span,
    });
    let done_pattern = Pattern::Variant(VariantPattern {
        enum_name: Some("Status".to_string()),
        variant_name: "Done".to_string(),
        binding: Some("value".to_string()),
        span,
    });
    let ready_variant = Value::EnumVariant(EnumVariantValue {
        enum_name: "Status".to_string(),
        variant_name: "Ready".to_string(),
        payload: None,
    });
    let done_variant = Value::EnumVariant(EnumVariantValue {
        enum_name: "Status".to_string(),
        variant_name: "Done".to_string(),
        payload: Some(Box::new(Value::Int(IntegerValue::from_signed(7)))),
    });
    assert_eq!(
        interpreter
            .match_pattern(&ready_pattern, &ready_variant)
            .expect("unit variants should match"),
        Some(None)
    );
    assert_eq!(
        interpreter
            .match_pattern(&ready_pattern, &done_variant)
            .expect("different variants should not match"),
        None
    );
    assert_eq!(
        interpreter
            .match_pattern(&done_pattern, &done_variant)
            .expect("payload variants should bind"),
        Some(Some((
            "value".to_string(),
            Value::Int(IntegerValue::from_signed(7)),
        )))
    );
    assert!(interpreter
        .match_pattern(&done_pattern, &Value::Bool(true))
        .expect_err("variant patterns should reject non-enum scrutinees")
        .message
        .contains("expected enum value for pattern"));
    assert!(interpreter
        .match_pattern(
            &Pattern::Variant(VariantPattern {
                enum_name: Some("Status".to_string()),
                variant_name: "Ready".to_string(),
                binding: Some("value".to_string()),
                span,
            }),
            &ready_variant,
        )
        .expect_err("payload shape mismatches should be rejected")
        .message
        .contains("payload shape did not match"));

    let mut env = Env::with_root();
    let if_error = match interpreter.exec_stmt(
        &Stmt::If(IfStmt {
            branches: vec![IfBranch {
                condition: expr(ExprKind::Int(1)),
                body: Vec::new(),
                span,
            }],
            else_body: None,
            span,
        }),
        &mut env,
    ) {
        Ok(_) => panic!("if conditions must stay boolean"),
        Err(error) => error,
    };
    assert!(if_error
        .message
        .contains("`if` condition must evaluate to `bool`"));
    let match_error = match interpreter.exec_stmt(
        &Stmt::Match(MatchStmt {
            scrutinee: expr(ExprKind::Bool(true)),
            borrow_mode: None,
            arms: vec![MatchArm {
                pattern: done_pattern,
                body: Vec::new(),
                span,
            }],
            span,
        }),
        &mut env,
    ) {
        Ok(_) => panic!("runtime match should surface variant-type mismatches"),
        Err(error) => error,
    };
    assert!(match_error
        .message
        .contains("expected enum value for pattern"));
    match interpreter
        .exec_stmt(
            &Stmt::Expr(ExprStmt {
                expr: expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Name("work".to_string()))),
                    args: vec![positional_arg(expr(ExprKind::Int(5)))],
                }),
                span,
            }),
            &mut env,
        )
        .expect("plain expression statements should continue")
    {
        ExecFlow::Continue => {}
        _ => panic!("expression statements should continue"),
    }
}

#[test]
fn interpreter_expression_inference_helpers_cover_builtin_and_runtime_member_surface() {
    let interpreter = test_interpreter(
            "trait Named:\n    def label(borrow self) -> String\n\nclass Boxed:\n    value: int32\n\n    def get(borrow self) -> int32:\n        return self.value\n\nimpl Named for Boxed:\n    def label(borrow self) -> String:\n        return \"box\"\n\nenum Status:\n    Ready\n\ndef helper() -> int32:\n    return 7\n\ndef worker() -> int32:\n    return 9\n\ndef main():\n    pass\n",
        );
    let mut env = Env::with_root();
    env.define_typed(
        "number".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(4)),
    );
    env.define_typed(
        "text".to_string(),
        Type::named("String"),
        Value::String("Aurora".to_string()),
    );
    env.define_typed(
        "values".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        }),
    );
    env.define_typed(
        "names".to_string(),
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ada".to_string())],
        }),
    );
    env.define_typed(
        "mapping".to_string(),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define_typed(
        "entry".to_string(),
        Type::Named(
            "MapEntry".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Unit,
    );
    env.define_typed(
        "jobs".to_string(),
        Type::Named("Channel".to_string(), vec![Type::named("int32")]),
        Value::Channel(ChannelValue::new()),
    );
    env.define_typed(
        "task".to_string(),
        Type::Named("Task".to_string(), vec![Type::named("int32")]),
        Value::Task(TaskValue::from_handle(thread::spawn(|| {
            Ok(Value::Int(IntegerValue::from_signed(9)))
        }))),
    );
    env.define_typed(
        "group".to_string(),
        Type::named("TaskGroup"),
        Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
    );
    env.define_typed(
        "parsed".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_ok(Value::Int(IntegerValue::from_signed(5))),
    );
    env.define_typed(
        "boxed".to_string(),
        Type::named("Boxed"),
        Value::Instance(InstanceValue {
            class_name: "Boxed".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(9)),
            )]),
        }),
    );

    for (expr, expected) in [
        (expr(ExprKind::Name("None".to_string())), Type::Unit),
        (
            expr(ExprKind::Name("helper".to_string())),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Name("Boxed".to_string())),
            Type::named("Boxed"),
        ),
        (
            expr(ExprKind::Name("Status".to_string())),
            Type::named("Status"),
        ),
        (expr(ExprKind::Int(1)), Type::named("int32")),
        (expr(ExprKind::DurationMillis(5)), Type::named("Duration")),
        (expr(ExprKind::Float(1.5)), Type::named("float64")),
        (expr(ExprKind::Bool(true)), Type::named("bool")),
        (
            expr(ExprKind::String("aurora".to_string())),
            Type::named("String"),
        ),
        (
            expr(ExprKind::List(vec![expr(ExprKind::Int(1))])),
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Set(vec![expr(ExprKind::String("a".to_string()))])),
            Type::Named("Set".to_string(), vec![Type::named("String")]),
        ),
        (
            expr(ExprKind::Map(vec![MapEntryExpr {
                key: expr(ExprKind::String("name".to_string())),
                value: expr(ExprKind::Int(1)),
            }])),
            Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
        ),
        (
            expr(ExprKind::FString(vec![FormatPart::Literal(
                "hi".to_string(),
            )])),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Vec".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Group(Box::new(expr(ExprKind::Name(
                "number".to_string(),
            ))))),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Cast {
                expr: Box::new(expr(ExprKind::Name("number".to_string()))),
                ty: type_ref("float64"),
            }),
            Type::named("float64"),
        ),
        (
            expr(ExprKind::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr(ExprKind::Bool(true))),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr(ExprKind::Int(5))),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Spawn {
                detached: true,
                value: Box::new(expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Name("worker".to_string()))),
                    args: Vec::new(),
                })),
            }),
            Type::Unit,
        ),
        (
            expr(ExprKind::Spawn {
                detached: false,
                value: Box::new(expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Name("worker".to_string()))),
                    args: Vec::new(),
                })),
            }),
            Type::Named("Task".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "parsed".to_string(),
            ))))),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Binary {
                op: BinaryOp::Eq,
                left: Box::new(expr(ExprKind::Name("number".to_string()))),
                right: Box::new(expr(ExprKind::Int(1))),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(expr(ExprKind::Name("text".to_string()))),
                right: Box::new(expr(ExprKind::String("!".to_string()))),
            }),
            Type::named("String"),
        ),
    ] {
        interpreter.expr_type_cache.borrow_mut().clear();
        assert_eq!(
            interpreter.infer_expr_type(&expr, &env),
            Some(expected),
            "unexpected inferred type for expression {expr:?}"
        );
    }

    for (expr, expected) in [
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("number".to_string()))),
                field: "to_string".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Float(1.5))),
                field: "to_string".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "len".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "is_empty".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "clone".to_string(),
            }),
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "pop".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "insert".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "swap".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "contains".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "extend".to_string(),
            }),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "clear".to_string(),
            }),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "reverse".to_string(),
            }),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "get".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                field: "set".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("names".to_string()))),
                field: "len".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("names".to_string()))),
                field: "is_empty".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("names".to_string()))),
                field: "clone".to_string(),
            }),
            Type::Named("Set".to_string(), vec![Type::named("String")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("names".to_string()))),
                field: "contains".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("names".to_string()))),
                field: "insert".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("names".to_string()))),
                field: "remove".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "len".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "is_empty".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "clone".to_string(),
            }),
            Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "contains_key".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "keys".to_string(),
            }),
            Type::Named("Vec".to_string(), vec![Type::named("String")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "get".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "set".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "remove".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "values".to_string(),
            }),
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "items".to_string(),
            }),
            Type::Named(
                "Vec".to_string(),
                vec![Type::Named(
                    "MapEntry".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                )],
            ),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "entries".to_string(),
            }),
            Type::Named(
                "Vec".to_string(),
                vec![Type::Named(
                    "MapEntry".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                )],
            ),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "clear".to_string(),
            }),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                field: "extend".to_string(),
            }),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "len".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "contains".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "starts_with".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "ends_with".to_string(),
            }),
            Type::named("bool"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "split".to_string(),
            }),
            Type::Named("Vec".to_string(), vec![Type::named("String")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "replace".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "to_lower".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "to_upper".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "trim".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "strip_prefix".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("String")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "strip_suffix".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("String")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "clone".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("text".to_string()))),
                field: "join".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("entry".to_string()))),
                field: "key".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("entry".to_string()))),
                field: "value".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("jobs".to_string()))),
                field: "recv".to_string(),
            }),
            Type::Named("Option".to_string(), vec![Type::named("int32")]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("jobs".to_string()))),
                field: "send".to_string(),
            }),
            Type::Named(
                "Result".to_string(),
                vec![
                    Type::Unit,
                    Type::Named("SendError".to_string(), vec![Type::named("int32")]),
                ],
            ),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("task".to_string()))),
                field: "join".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("group".to_string()))),
                field: "spawn".to_string(),
            }),
            Type::Named("Task".to_string(), vec![Type::Unit]),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("group".to_string()))),
                field: "cancel".to_string(),
            }),
            Type::Unit,
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("boxed".to_string()))),
                field: "value".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("boxed".to_string()))),
                field: "get".to_string(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("boxed".to_string()))),
                field: "label".to_string(),
            }),
            Type::named("String"),
        ),
        (
            expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Name("values".to_string()))),
                index: Box::new(expr(ExprKind::Int(0))),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                index: Box::new(expr(ExprKind::String("count".to_string()))),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_int32".to_string()))),
                args: vec![named_arg("text", expr(ExprKind::String("7".to_string())))],
            }),
            Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")],
            ),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("helper".to_string()))),
                args: Vec::new(),
            }),
            Type::named("int32"),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("Boxed".to_string()))),
                args: Vec::new(),
            }),
            Type::named("Boxed"),
        ),
    ] {
        interpreter.expr_type_cache.borrow_mut().clear();
        assert_eq!(
            interpreter.infer_expr_type(&expr, &env),
            Some(expected),
            "unexpected inferred member/call type for expression {expr:?}"
        );
    }

    for (expr, expected) in [
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("print".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            Some(Type::Unit),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("range".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(3)))],
            }),
            Some(Type::named("Range")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("task_group".to_string()))),
                args: Vec::new(),
            }),
            Some(Type::named("TaskGroup")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("cancelled".to_string()))),
                args: Vec::new(),
            }),
            Some(Type::named("bool")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("after".to_string()))),
                args: vec![positional_arg(expr(ExprKind::DurationMillis(5)))],
            }),
            Some(Type::named("Duration")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("sleep".to_string()))),
                args: vec![positional_arg(expr(ExprKind::DurationMillis(5)))],
            }),
            Some(Type::Unit),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("abs".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Float(3.5)))],
            }),
            Some(Type::named("float64")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("min".to_string()))),
                args: vec![
                    positional_arg(expr(ExprKind::Int(1))),
                    positional_arg(expr(ExprKind::Int(2))),
                ],
            }),
            Some(Type::named("int32")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("max".to_string()))),
                args: vec![
                    positional_arg(expr(ExprKind::Float(1.0))),
                    positional_arg(expr(ExprKind::Float(2.0))),
                ],
            }),
            Some(Type::named("float64")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("sqrt".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Float(9.0)))],
            }),
            Some(Type::named("float64")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_int64".to_string()))),
                args: vec![positional_arg(expr(ExprKind::String("7".to_string())))],
            }),
            Some(Type::Named(
                "Result".to_string(),
                vec![Type::named("int64"), Type::named("String")],
            )),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_float64".to_string()))),
                args: vec![positional_arg(expr(ExprKind::String("7.5".to_string())))],
            }),
            Some(Type::Named(
                "Result".to_string(),
                vec![Type::named("float64"), Type::named("String")],
            )),
        ),
    ] {
        interpreter.expr_type_cache.borrow_mut().clear();
        assert_eq!(interpreter.infer_expr_type(&expr, &env), expected);
    }

    let channel_call = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Name("channel".to_string()))),
        args: Vec::new(),
    });
    interpreter.expr_type_cache.borrow_mut().clear();
    assert_eq!(interpreter.infer_expr_type(&channel_call, &env), None);
}

#[test]
fn channel_task_and_group_identity_and_error_paths_are_stable() {
    let channel = ChannelValue::new();
    let channel_clone = channel.clone();
    assert_eq!(channel, channel_clone);
    channel.close();
    assert_eq!(channel.recv_blocking(), None);

    let panic_task =
        TaskValue::from_handle(thread::spawn(|| -> std::result::Result<Value, String> {
            panic!("boom");
        }));
    let join_error = panic_task
        .join_result()
        .expect_err("panicking task should surface an error");
    assert!(join_error.contains("panicked"));

    let group = TaskGroupValue::new(&CancellationContext::default());
    let group_clone = group.clone();
    assert_eq!(group, group_clone);

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let context = CancellationContext {
        flags: vec![cancel_flag.clone()],
    };
    assert!(!context.is_cancelled());
    cancel_flag.store(true, Ordering::SeqCst);
    assert!(context.is_cancelled());
}

#[test]
fn channel_close_wakes_blocked_receivers_and_task_join_reports_missing_handle() {
    let channel = ChannelValue::new();
    let waiting = channel.clone();
    let handle = thread::spawn(move || waiting.recv_blocking());
    thread::sleep(std::time::Duration::from_millis(5));
    channel.close();
    assert_eq!(handle.join().expect("receiver thread should join"), None);

    let missing_handle = TaskValue {
        inner: Arc::new(super::TaskState {
            handle: Mutex::new(super::TaskHandle::Running(None)),
        }),
    };
    let error = missing_handle
        .join_result()
        .expect_err("missing handle should surface a task join error");
    assert!(error.contains("task join handle was not available"));
}

#[test]
fn interpreter_inference_and_runtime_helpers_cover_remaining_edge_paths() {
    let interpreter = test_interpreter(
            "class Box[T]:\n    value: T\n\nclass Plain:\n    value: int32\n\ndef helper() -> int32:\n    return 7\n\ndef main():\n    pass\n",
        );

    let open_channel = ChannelValue::new();
    assert!(matches!(open_channel.try_recv(), TryRecvResult::Empty));
    open_channel
        .send(Value::Int(IntegerValue::from_signed(3)))
        .expect("open channels should accept sends");
    match open_channel.try_recv() {
        TryRecvResult::Value(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(3));
        }
        TryRecvResult::Value(other) => {
            panic!("expected integer channel value, found {other:?}")
        }
        TryRecvResult::Closed => panic!("expected queued channel value, found closed channel"),
        TryRecvResult::Empty => panic!("expected queued channel value, found empty channel"),
    }
    open_channel.close();
    assert!(matches!(open_channel.try_recv(), TryRecvResult::Closed));
    assert_eq!(
        open_channel.send(Value::Bool(true)),
        Err(Value::Bool(true)),
        "closed channels should reject sends and return the original value",
    );

    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Bool(true))));
    assert_eq!(
        task.join_result().expect("task should join"),
        Value::Bool(true)
    );
    assert_eq!(
        task.join_result()
            .expect("completed tasks should be reusable"),
        Value::Bool(true)
    );

    assert_eq!(
        interpreter.infer_instance_type(&InstanceValue {
            class_name: "Box".to_string(),
            fields: BTreeMap::from([("value".to_string(), Value::String("aurora".to_string()),)]),
        }),
        Some(Type::Named("Box".to_string(), vec![Type::named("String")])),
    );
    assert_eq!(
        interpreter.infer_instance_type(&InstanceValue {
            class_name: "Box".to_string(),
            fields: BTreeMap::new(),
        }),
        None
    );
    assert_eq!(
        interpreter.infer_instance_type(&InstanceValue {
            class_name: "Missing".to_string(),
            fields: BTreeMap::new(),
        }),
        None
    );

    assert_eq!(
        interpreter.infer_runtime_value_type(&option_none()),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("Unknown")]
        )),
    );
    assert_eq!(
        interpreter.infer_runtime_value_type(&result_ok(Value::Int(IntegerValue::from_signed(9)))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("Unknown")],
        )),
    );
    assert_eq!(
        interpreter.infer_runtime_value_type(&send_error_closed(Value::Bool(true))),
        Some(Type::Named(
            "SendError".to_string(),
            vec![Type::named("bool")],
        )),
    );

    let mut env = Env::with_root();
    env.define_typed(
        "count".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    env.define_typed(
        "ready".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_ok(Value::Int(IntegerValue::from_signed(5))),
    );

    for (expr, expected) in [
        (
            expr(ExprKind::Spawn {
                detached: false,
                value: Box::new(expr(ExprKind::Name("count".to_string()))),
            }),
            None,
        ),
        (
            expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "count".to_string(),
            ))))),
            None,
        ),
        (
            expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "ready".to_string(),
            ))))),
            Some(Type::named("int32")),
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("helper".to_string()))),
                args: Vec::new(),
            }),
            Some(Type::named("int32")),
        ),
    ] {
        interpreter.expr_type_cache.borrow_mut().clear();
        assert_eq!(interpreter.infer_expr_type(&expr, &env), expected);
    }
}

#[test]
fn interpreter_collection_string_and_task_helpers_cover_remaining_runtime_paths() {
    let mut interpreter =
        test_interpreter("def helper() -> int32:\n    return 7\n\ndef main():\n    pass\n");
    let span = Span::new(1, 1);
    let receiver = expr(ExprKind::Name("values".to_string()));
    let mut env = Env::with_root();
    env.define_typed(
        "values".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "texts".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: vec![
                Value::String("one".to_string()),
                Value::String("two".to_string()),
            ],
        }),
    );
    env.define_typed(
        "other".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(3)),
                Value::Int(IntegerValue::from_signed(4)),
            ],
        }),
    );
    env.define_typed(
        "bad_texts".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: vec![Value::Bool(true)],
        }),
    );
    env.define_typed(
        "mapping".to_string(),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define_typed(
        "mapping_other".to_string(),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![
                (
                    Value::String("count".to_string()),
                    Value::Int(IntegerValue::from_signed(5)),
                ),
                (
                    Value::String("next".to_string()),
                    Value::Int(IntegerValue::from_signed(9)),
                ),
            ],
        }),
    );
    env.define_typed(
        "flags".to_string(),
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        }),
    );

    let inserted = interpreter
        .eval_vec_method(
            match env.get("values").cloned().unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "insert",
            &receiver,
            &[
                named_arg("index", expr(ExprKind::Int(1))),
                named_arg("value", expr(ExprKind::Int(99))),
            ],
            &mut env,
            span,
        )
        .expect("vec insert should succeed");
    assert_eq!(expect_value_outcome(inserted), Value::Bool(true));

    let contains = interpreter
        .eval_vec_method(
            match env.get("values").cloned().unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "contains",
            &receiver,
            &[positional_arg(expr(ExprKind::Int(99)))],
            &mut env,
            span,
        )
        .expect("vec contains should succeed");
    assert_eq!(expect_value_outcome(contains), Value::Bool(true));

    let swapped = interpreter
        .eval_vec_method(
            match env.get("values").cloned().unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "swap",
            &receiver,
            &[
                positional_arg(expr(ExprKind::Int(0))),
                positional_arg(expr(ExprKind::Int(2))),
            ],
            &mut env,
            span,
        )
        .expect("vec swap should succeed");
    assert_eq!(expect_value_outcome(swapped), Value::Bool(true));

    let reversed = interpreter
        .eval_vec_method(
            match env.get("values").cloned().unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "reverse",
            &receiver,
            &[],
            &mut env,
            span,
        )
        .expect("vec reverse should succeed");
    assert_eq!(expect_value_outcome(reversed), Value::Unit);

    let extended = interpreter
        .eval_vec_method(
            match env.get("values").cloned().unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "extend",
            &receiver,
            &[positional_arg(expr(ExprKind::Name("other".to_string())))],
            &mut env,
            span,
        )
        .expect("vec extend should succeed");
    assert_eq!(expect_value_outcome(extended), Value::Unit);

    let cleared = interpreter
        .eval_vec_method(
            match env.get("values").cloned().unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "clear",
            &receiver,
            &[],
            &mut env,
            span,
        )
        .expect("vec clear should succeed");
    assert_eq!(expect_value_outcome(cleared), Value::Unit);

    let vec_method_error = expect_eval_error(
        interpreter.eval_vec_method(
            match env.get("texts").cloned().unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "mystery",
            &expr(ExprKind::Name("texts".to_string())),
            &[],
            &mut env,
            span,
        ),
        "unsupported vec method should fail",
    );
    assert!(vec_method_error
        .message
        .contains("unsupported vector method"));

    let map_receiver = expr(ExprKind::Name("mapping".to_string()));
    let items = interpreter
        .eval_map_method(
            match env.get("mapping").cloned().unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "items",
            &map_receiver,
            &[],
            &mut env,
            span,
        )
        .expect("map items should succeed");
    match expect_value_outcome(items) {
        Value::Vec(values) => assert_eq!(values.elements.len(), 1),
        other => panic!("expected vec, found {other:?}"),
    }

    let extend_map = interpreter
        .eval_map_method(
            match env.get("mapping").cloned().unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "extend",
            &map_receiver,
            &[positional_arg(expr(ExprKind::Name(
                "mapping_other".to_string(),
            )))],
            &mut env,
            span,
        )
        .expect("map extend should succeed");
    assert_eq!(expect_value_outcome(extend_map), Value::Unit);

    let clear_map = interpreter
        .eval_map_method(
            match env.get("mapping").cloned().unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "clear",
            &map_receiver,
            &[],
            &mut env,
            span,
        )
        .expect("map clear should succeed");
    assert_eq!(expect_value_outcome(clear_map), Value::Unit);

    let map_method_error = expect_eval_error(
        interpreter.eval_map_method(
            match env.get("mapping_other").cloned().unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "extend",
            &expr(ExprKind::Name("mapping_other".to_string())),
            &[positional_arg(expr(ExprKind::Int(7)))],
            &mut env,
            span,
        ),
        "map extend should reject non-map values",
    );
    assert!(map_method_error
        .message
        .contains("requires another `Map[K, V]` value"));

    let set_receiver = expr(ExprKind::Name("flags".to_string()));
    let inserted_flag = interpreter
        .eval_set_method(
            match env.get("flags").cloned().unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "insert",
            &set_receiver,
            &[positional_arg(expr(ExprKind::String("go".to_string())))],
            &mut env,
            span,
        )
        .expect("set insert should succeed");
    assert_eq!(expect_value_outcome(inserted_flag), Value::Bool(true));

    let removed_flag = interpreter
        .eval_set_method(
            match env.get("flags").cloned().unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "remove",
            &set_receiver,
            &[positional_arg(expr(ExprKind::String("ready".to_string())))],
            &mut env,
            span,
        )
        .expect("set remove should succeed");
    assert_eq!(expect_value_outcome(removed_flag), Value::Bool(true));

    let set_method_error = expect_eval_error(
        interpreter.eval_set_method(
            match env.get("flags").cloned().unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "unknown",
            &set_receiver,
            &[],
            &mut env,
            span,
        ),
        "unsupported set method should fail",
    );
    assert!(set_method_error.message.contains("unsupported set method"));

    let string_contains = interpreter
        .eval_string_method(
            "aurora".to_string(),
            "contains",
            &[positional_arg(expr(ExprKind::String("ror".to_string())))],
            &mut env,
            span,
        )
        .expect("string contains should succeed");
    assert_eq!(expect_value_outcome(string_contains), Value::Bool(true));

    let string_split = interpreter
        .eval_string_method(
            "a,b".to_string(),
            "split",
            &[positional_arg(expr(ExprKind::String(",".to_string())))],
            &mut env,
            span,
        )
        .expect("string split should succeed");
    match expect_value_outcome(string_split) {
        Value::Vec(parts) => assert_eq!(parts.elements.len(), 2),
        other => panic!("expected split vec, found {other:?}"),
    }

    let string_join = interpreter
        .eval_string_method(
            ", ".to_string(),
            "join",
            &[positional_arg(expr(ExprKind::Name("texts".to_string())))],
            &mut env,
            span,
        )
        .expect("string join should succeed");
    assert_eq!(
        expect_value_outcome(string_join),
        Value::String("one, two".to_string())
    );

    let string_strip = interpreter
        .eval_string_method(
            "prefix-value".to_string(),
            "strip_prefix",
            &[positional_arg(expr(ExprKind::String(
                "prefix-".to_string(),
            )))],
            &mut env,
            span,
        )
        .expect("string strip_prefix should succeed");
    match expect_value_outcome(string_strip) {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "Some"),
        other => panic!("expected option result, found {other:?}"),
    }
    let string_strip_none = interpreter
        .eval_string_method(
            "prefix-value".to_string(),
            "strip_prefix",
            &[positional_arg(expr(ExprKind::String(
                "missing".to_string(),
            )))],
            &mut env,
            span,
        )
        .expect("string strip_prefix should return Option.None for no match");
    match expect_value_outcome(string_strip_none) {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "None"),
        other => panic!("expected Option.None, found {other:?}"),
    }
    let string_suffix_none = interpreter
        .eval_string_method(
            "prefix-value".to_string(),
            "strip_suffix",
            &[positional_arg(expr(ExprKind::String(
                "missing".to_string(),
            )))],
            &mut env,
            span,
        )
        .expect("string strip_suffix should return Option.None for no match");
    match expect_value_outcome(string_suffix_none) {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "None"),
        other => panic!("expected Option.None, found {other:?}"),
    }

    let string_error = expect_eval_error(
        interpreter.eval_string_method(
            "aurora".to_string(),
            "contains",
            &[positional_arg(expr(ExprKind::Bool(true)))],
            &mut env,
            span,
        ),
        "string contains should reject non-strings",
    );
    assert!(string_error.message.contains("requires a `String`"));
    let split_error = expect_eval_error(
        interpreter.eval_string_method(
            "a,b".to_string(),
            "split",
            &[positional_arg(expr(ExprKind::Bool(true)))],
            &mut env,
            span,
        ),
        "string split should reject non-strings",
    );
    assert!(split_error.message.contains("requires a `String`"));
    let replace_error = expect_eval_error(
        interpreter.eval_string_method(
            "aurora".to_string(),
            "replace",
            &[
                positional_arg(expr(ExprKind::String("au".to_string()))),
                positional_arg(expr(ExprKind::Bool(true))),
            ],
            &mut env,
            span,
        ),
        "string replace should reject non-string replacements",
    );
    assert!(replace_error.message.contains("requires `String` for `to`"));

    let join_error = expect_eval_error(
        interpreter.eval_string_method(
            ", ".to_string(),
            "join",
            &[positional_arg(expr(ExprKind::Int(1)))],
            &mut env,
            span,
        ),
        "string join should reject non-vectors",
    );
    assert!(join_error.message.contains("requires `Vec[String]`"));
    let join_element_error = expect_eval_error(
        interpreter.eval_string_method(
            ", ".to_string(),
            "join",
            &[positional_arg(expr(ExprKind::Name(
                "bad_texts".to_string(),
            )))],
            &mut env,
            span,
        ),
        "string join should reject non-string vec elements",
    );
    assert!(join_element_error
        .message
        .contains("requires `Vec[String]`"));

    let shared_task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Bool(true))));
    let task_clone = interpreter
        .eval_task_method(
            TaskValue::from_handle(thread::spawn(|| {
                Ok(Value::Int(IntegerValue::from_signed(7)))
            })),
            "clone",
            &[],
            span,
        )
        .expect("task clone should succeed");
    match expect_value_outcome(task_clone) {
        Value::Task(_) => {}
        other => panic!("expected task, found {other:?}"),
    }

    let task_join = interpreter
        .eval_task_method(shared_task.clone(), "join", &[], span)
        .expect("task join should succeed");
    assert_eq!(expect_value_outcome(task_join), Value::Bool(true));
    let cached_task_join = interpreter
        .eval_task_method(shared_task, "join", &[], span)
        .expect("completed task joins should use the cached result");
    assert_eq!(expect_value_outcome(cached_task_join), Value::Bool(true));

    let task_error = expect_eval_error(
        interpreter.eval_task_method(
            TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit))),
            "cancel",
            &[],
            span,
        ),
        "unsupported task method should fail",
    );
    assert!(task_error.message.contains("unsupported task method"));

    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancelled = interpreter
        .eval_task_group_method(group.clone(), "cancel", &[], &mut env, span)
        .expect("task-group cancel should succeed");
    assert_eq!(expect_value_outcome(cancelled), Value::Unit);

    let no_target = expect_eval_error(
        interpreter.eval_task_group_method(group.clone(), "spawn", &[], &mut env, span),
        "task-group spawn should reject missing targets",
    );
    assert!(no_target.message.contains("expects a target function"));

    let bad_target = expect_eval_error(
        interpreter.eval_task_group_method(
            group.clone(),
            "spawn",
            &[positional_arg(expr(ExprKind::Int(3)))],
            &mut env,
            span,
        ),
        "task-group spawn should require named functions",
    );
    assert!(bad_target.message.contains("named function target"));

    let unknown_target = expect_eval_error(
        interpreter.eval_task_group_method(
            group,
            "spawn",
            &[positional_arg(expr(ExprKind::Name("missing".to_string())))],
            &mut env,
            span,
        ),
        "task-group spawn should report unknown functions",
    );
    assert!(unknown_target
        .message
        .contains("unknown function `missing`"));
}

#[test]
fn interpreter_index_and_assign_helpers_cover_error_paths() {
    let mut interpreter = test_interpreter("def main():\n    pass\n");
    let span = Span::new(2, 3);
    let mut env = Env::with_root();
    env.define_typed(
        "values".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(5))],
        }),
    );
    env.define_typed(
        "mapping".to_string(),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );

    let negative = interpreter
        .evaluate_index_expr(&expr(ExprKind::Int(u128::MAX)), &mut env, span)
        .expect_err("out-of-range integer literal should fail as an index");
    assert!(negative.message.contains("supported signed range"));

    let non_integer = interpreter
        .evaluate_index_expr(&expr(ExprKind::Bool(true)), &mut env, span)
        .expect_err("non-integer index should fail");
    assert!(non_integer
        .message
        .contains("vector indices must be integers"));

    let missing_vec = interpreter
        .read_vector_element(
            &VecValue {
                element_type: Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            },
            3,
            span,
        )
        .expect_err("out-of-bounds vector read should fail");
    assert!(missing_vec.message.contains("out of bounds"));

    let missing_key = interpreter
        .read_map_element(
            &MapValue {
                key_type: Type::named("String"),
                value_type: Type::named("int32"),
                entries: vec![(
                    Value::String("count".to_string()),
                    Value::Int(IntegerValue::from_signed(1)),
                )],
            },
            &Value::String("missing".to_string()),
            span,
        )
        .expect_err("missing map key should fail");
    assert!(missing_key
        .message
        .contains("map key `missing` was not present"));

    let wrong_target = interpreter
        .read_assign_target(
            &AssignTarget::Index {
                object: Box::new(expr(ExprKind::Bool(true))),
                index: Box::new(expr(ExprKind::Int(0))),
            },
            &mut env,
            span,
        )
        .expect_err("indexing a non-vector-or-map should fail");
    assert!(wrong_target
        .message
        .contains("expression is not a mutable place"));
}

#[test]
fn interpreter_cleanup_and_writeback_helpers_cover_resource_and_borrow_paths() {
    let mut interpreter = test_interpreter(
            "class Resource:\n    closed: bool\n\n    def close(borrow mut self):\n        self.closed = true\n\nclass Worker:\n    value: int32\n\ndef main():\n    pass\n",
        );
    let span = Span::new(2, 3);
    let mut env = Env::with_root();
    env.define_typed(
        "resource".to_string(),
        Type::named("Resource"),
        Value::Instance(InstanceValue {
            class_name: "Resource".to_string(),
            fields: BTreeMap::from([("closed".to_string(), Value::Bool(false))]),
        }),
    );
    interpreter
        .run_with_cleanup("resource", &mut env, span, false)
        .expect("resource cleanup should call close and write back the receiver");
    match env.get("resource") {
        Some(Value::Instance(instance)) => {
            assert_eq!(instance.fields.get("closed"), Some(&Value::Bool(true)));
        }
        other => panic!("expected updated resource instance, found {other:?}"),
    }

    env.define_typed(
        "count".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    let non_resource = interpreter
        .run_with_cleanup("count", &mut env, span, false)
        .expect_err("non-resource cleanup targets should fail");
    assert!(non_resource
        .message
        .contains("with binding `count` is not a resource instance"));

    env.define_typed(
        "worker".to_string(),
        Type::named("Worker"),
        Value::Instance(InstanceValue {
            class_name: "Worker".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(9)),
            )]),
        }),
    );
    let missing_close = interpreter
        .run_with_cleanup("worker", &mut env, span, false)
        .expect_err("classes without close should fail");
    assert!(missing_close
        .message
        .contains("cannot be used with `with` because it has no `close` method"));

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let child = group.child_cancellation();
    group.register_task(TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit))));
    env.define_typed(
        "group".to_string(),
        Type::named("TaskGroup"),
        Value::TaskGroup(group.clone()),
    );
    interpreter
        .run_with_cleanup("group", &mut env, span, true)
        .expect("task-group cleanup should cancel and join tracked tasks");
    assert!(child.is_cancelled());
    assert!(group.drain_tasks().is_empty());

    env.define_typed(
        "target".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    let source_argument = positional_arg(expr(ExprKind::Name("count".to_string())));
    let target_argument = positional_arg(expr(ExprKind::Name("target".to_string())));
    let params = vec![
        Param {
            name: "source".to_string(),
            ty: type_ref("int32"),
            passing: ReceiverKind::Borrow,
            default: None,
            span,
        },
        Param {
            name: "target".to_string(),
            ty: type_ref("int32"),
            passing: ReceiverKind::BorrowMut,
            default: None,
            span,
        },
    ];
    let evaluated_args = vec![
        EvaluatedArg {
            argument: Some(&source_argument),
            value: Value::Int(IntegerValue::from_signed(1)),
        },
        EvaluatedArg {
            argument: Some(&target_argument),
            value: Value::Int(IntegerValue::from_signed(1)),
        },
    ];
    interpreter
        .apply_borrowed_param_writebacks(
            &params,
            &evaluated_args,
            &[
                (0, Value::Int(IntegerValue::from_signed(4))),
                (1, Value::Int(IntegerValue::from_signed(7))),
            ],
            &mut env,
        )
        .expect("borrow-mut writebacks should update explicit argument places");
    assert_eq!(
        env.get("target"),
        Some(&Value::Int(IntegerValue::from_signed(7)))
    );

    let missing_explicit_argument = interpreter
        .apply_borrowed_param_writebacks(
            &params,
            &[
                EvaluatedArg {
                    argument: None,
                    value: Value::Int(IntegerValue::from_signed(1)),
                },
                EvaluatedArg {
                    argument: None,
                    value: Value::Int(IntegerValue::from_signed(1)),
                },
            ],
            &[(1, Value::Int(IntegerValue::from_signed(9)))],
            &mut env,
        )
        .expect_err("borrow-mut writebacks require explicit arguments");
    assert!(missing_explicit_argument
        .message
        .contains("mutable borrowed parameter `target` requires an explicit argument"));
}

#[test]
fn runtime_render_and_cast_helpers_cover_remaining_value_variants() {
    let channel = ChannelValue::new();
    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    let group = TaskGroupValue::new(&CancellationContext::default());

    assert_eq!(format!("{channel:?}"), "ChannelValue(..)");
    assert_eq!(format!("{task:?}"), "TaskValue(..)");
    assert_eq!(format!("{group:?}"), "TaskGroupValue(..)");

    assert_eq!(
        Value::Set(SetValue {
            element_type: Type::named("bool"),
            elements: vec![Value::Bool(true), Value::Bool(false)],
        })
        .render(),
        "Set{true, false}"
    );
    assert_eq!(
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            )],
        })
        .render(),
        "{count: 2}"
    );
    assert_eq!(Value::Duration(5).render(), "5ms");
    assert_eq!(
        Value::Range(RangeValue { start: 1, end: 3 }).render(),
        "range(1, 3)"
    );
    assert_eq!(
        Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        })
        .render(),
        "<module pkg.tools>"
    );
    assert_eq!(Value::Unit.render(), "");
    assert_eq!(Value::Channel(channel.clone()).render(), "<channel>");
    assert_eq!(Value::Task(task.clone()).render(), "<task>");
    assert_eq!(Value::TaskGroup(group.clone()).render(), "<task_group>");
    assert_eq!(
        Value::Instance(InstanceValue {
            class_name: "Point".to_string(),
            fields: BTreeMap::from([("x".to_string(), Value::Int(IntegerValue::from_signed(1)),)]),
        })
        .render(),
        "Point(x=1)"
    );
    assert_eq!(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payload: Some(Box::new(Value::String("ok".to_string()))),
        })
        .render(),
        "Status.Ready(ok)"
    );

    for (value, source_name) in [
        (
            Value::Vec(VecValue {
                element_type: Type::named("int32"),
                elements: Vec::new(),
            }),
            "Vec",
        ),
        (
            Value::Set(SetValue {
                element_type: Type::named("String"),
                elements: Vec::new(),
            }),
            "Set",
        ),
        (
            Value::Map(MapValue {
                key_type: Type::named("String"),
                value_type: Type::named("int32"),
                entries: Vec::new(),
            }),
            "Map",
        ),
        (Value::Duration(9), "Duration"),
        (Value::Range(RangeValue { start: 0, end: 1 }), "Range"),
        (
            Value::ModuleNamespace(ModuleNamespaceValue {
                path: "pkg.tools".to_string(),
            }),
            "module pkg.tools",
        ),
        (Value::Unit, "None"),
        (
            Value::Instance(InstanceValue {
                class_name: "Counter".to_string(),
                fields: BTreeMap::new(),
            }),
            "Counter",
        ),
        (
            Value::EnumVariant(EnumVariantValue {
                enum_name: "Status".to_string(),
                variant_name: "Ready".to_string(),
                payload: None,
            }),
            "Status",
        ),
        (Value::Channel(channel.clone()), "Channel"),
        (Value::Task(task), "Task"),
        (Value::TaskGroup(group), "TaskGroup"),
    ] {
        let error = cast_numeric_value(value, &Type::named("int32"), None)
            .expect_err("non-numeric cast should fail");
        assert!(
            error
                .message
                .contains(&format!("found `{}` and `int32`", source_name)),
            "unexpected cast error for {source_name}: {}",
            error.message
        );
    }
}

#[test]
fn interpreter_callable_arg_and_spawn_helpers_cover_defaults_and_borrow_rules() {
    let source = [
        "def sample(value: int32, extra: int32 = 4) -> int32:",
        "    return value + extra",
        "",
        "def borrowed(value: borrow int32) -> int32:",
        "    return value",
        "",
        "def main() -> int32:",
        "    return 0",
    ]
    .join("\n");
    let mut interpreter = test_interpreter(&source);
    let mut env = Env::with_root();
    env.define_typed(
        "count".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(7)),
    );

    let sample = interpreter
        .program
        .functions
        .get("sample")
        .expect("sample function should exist")
        .decl
        .clone();
    let borrowed = interpreter
        .program
        .functions
        .get("borrowed")
        .expect("borrowed function should exist")
        .decl
        .clone();
    let args = [named_arg(
        "value",
        expr(ExprKind::Name("count".to_string())),
    )];

    let evaluated = interpreter
        .eval_callable_args("sample", &sample.params, &args, &mut env, Span::new(1, 1))
        .expect("callable args should evaluate");
    assert_eq!(evaluated.len(), 2);
    assert_eq!(evaluated[0].value, Value::Int(IntegerValue::from_signed(7)));
    assert!(evaluated[0].argument.is_some());
    assert_eq!(evaluated[1].value, Value::Int(IntegerValue::from_signed(4)));
    assert!(evaluated[1].argument.is_none());

    interpreter
        .require_spawnable_function(&sample, Span::new(1, 1))
        .expect("by-value functions should be spawnable");
    let spawn_error = interpreter
        .require_spawnable_function(&borrowed, Span::new(1, 1))
        .expect_err("borrowed params should not be spawnable");
    assert!(spawn_error
        .message
        .contains("does not yet support borrowed parameter `value`"));
}

#[test]
fn interpreter_builtin_call_surface_covers_named_and_error_paths() {
    let mut interpreter = test_interpreter("def main() -> int32:\n    return 0\n");
    let mut env = Env::with_root();
    env.define_typed(
        "delay".to_string(),
        Type::named("Duration"),
        Value::Duration(0),
    );
    env.define_typed(
        "negative_delay".to_string(),
        Type::named("Duration"),
        Value::Duration(-1),
    );
    env.define_typed(
        "neg".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(-4)),
    );
    env.define_typed(
        "text".to_string(),
        Type::named("String"),
        Value::String("12".to_string()),
    );
    env.define_typed(
        "word".to_string(),
        Type::named("String"),
        Value::String("Aurora".to_string()),
    );

    let eval_value = |interpreter: &mut Interpreter, expr: Expr, env: &mut Env| match interpreter
        .eval_expr(&expr, env)
        .expect("builtin call should evaluate")
    {
        EvalOutcome::Value(value) => value,
        EvalOutcome::Return(value) => panic!("unexpected return flow: {}", value.render()),
    };

    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("range".to_string()))),
                args: vec![named_arg("stop", expr(ExprKind::Int(3)))],
            }),
            &mut env,
        ),
        Value::Range(RangeValue { start: 0, end: 3 })
    );
    assert!(matches!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("channel".to_string()))),
                args: Vec::new(),
            }),
            &mut env,
        ),
        Value::Channel(_)
    ));
    assert!(matches!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("task_group".to_string()))),
                args: Vec::new(),
            }),
            &mut env,
        ),
        Value::TaskGroup(_)
    ));
    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("cancelled".to_string()))),
                args: Vec::new(),
            }),
            &mut env,
        ),
        Value::Bool(false)
    );
    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("after".to_string()))),
                args: vec![named_arg(
                    "duration",
                    expr(ExprKind::Name("delay".to_string())),
                )],
            }),
            &mut env,
        ),
        Value::Duration(0)
    );
    let after_error = match interpreter.eval_expr(
        &expr(ExprKind::Call {
            callee: Box::new(expr(ExprKind::Name("after".to_string()))),
            args: vec![named_arg("duration", expr(ExprKind::Bool(true)))],
        }),
        &mut env,
    ) {
        Ok(_) => panic!("after() should reject non-duration values"),
        Err(error) => error,
    };
    assert!(after_error.message.contains("expects a `Duration`"));

    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("sleep".to_string()))),
                args: vec![named_arg(
                    "duration",
                    expr(ExprKind::Name("delay".to_string())),
                )],
            }),
            &mut env,
        ),
        Value::Unit
    );
    let sleep_error = match interpreter.eval_expr(
        &expr(ExprKind::Call {
            callee: Box::new(expr(ExprKind::Name("sleep".to_string()))),
            args: vec![named_arg(
                "duration",
                expr(ExprKind::Name("negative_delay".to_string())),
            )],
        }),
        &mut env,
    ) {
        Ok(_) => panic!("negative sleep durations should fail"),
        Err(error) => error,
    };
    assert!(sleep_error
        .message
        .contains("does not fit in the runtime timer range"));

    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("abs".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Name("neg".to_string())))],
            }),
            &mut env,
        ),
        Value::Int(IntegerValue::from_signed(4))
    );
    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("min".to_string()))),
                args: vec![
                    positional_arg(expr(ExprKind::Int(8))),
                    positional_arg(expr(ExprKind::Int(3))),
                ],
            }),
            &mut env,
        ),
        Value::Int(IntegerValue::from_signed(3))
    );
    let min_error = match interpreter.eval_expr(
        &expr(ExprKind::Call {
            callee: Box::new(expr(ExprKind::Name("min".to_string()))),
            args: vec![
                positional_arg(expr(ExprKind::Int(1))),
                positional_arg(expr(ExprKind::Bool(true))),
            ],
        }),
        &mut env,
    ) {
        Ok(_) => panic!("min() should reject mismatched types"),
        Err(error) => error,
    };
    assert!(min_error
        .message
        .contains("expects matching numeric arguments"));
    let sqrt_error = match interpreter.eval_expr(
        &expr(ExprKind::Call {
            callee: Box::new(expr(ExprKind::Name("sqrt".to_string()))),
            args: vec![positional_arg(expr(ExprKind::Int(9)))],
        }),
        &mut env,
    ) {
        Ok(_) => panic!("sqrt() should reject integer operands"),
        Err(error) => error,
    };
    assert!(sqrt_error
        .message
        .contains("expects `float32` or `float64`"));

    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_int32".to_string()))),
                args: vec![named_arg("text", expr(ExprKind::Name("text".to_string())))],
            }),
            &mut env,
        ),
        result_ok(Value::Int(IntegerValue::from_signed(12)))
    );
    assert_eq!(
        eval_value(
            &mut interpreter,
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_float64".to_string()))),
                args: vec![named_arg("text", expr(ExprKind::Name("word".to_string())))],
            }),
            &mut env,
        ),
        result_err(Value::String("invalid float literal".to_string()))
    );
}

#[test]
fn interpreter_eval_expr_specialized_collection_and_try_edges_cover_remaining_branches() {
    let mut interpreter = test_interpreter(
            "class Counter:\n    value: int32\n\nenum Status:\n    Ready\n    Done(int32)\n\ndef echo(value: int32) -> int32:\n    return value\n\ndef main():\n    pass\n",
        );
    let mut env = Env::with_root();
    env.define_typed(
        "err_result".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_err(Value::String("boom".to_string())),
    );
    env.define_typed(
        "ok_result".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_ok(Value::Int(IntegerValue::from_signed(4))),
    );
    env.define_typed(
        "broken_result".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Result".to_string(),
            variant_name: "Ok".to_string(),
            payload: None,
        }),
    );
    env.define_typed(
        "unsigned_count".to_string(),
        Type::named("uint32"),
        Value::Int(IntegerValue::Unsigned(7)),
    );

    let expect_value = |outcome: EvalOutcome| match outcome {
        EvalOutcome::Value(value) => value,
        EvalOutcome::Return(_) => panic!("expected ordinary value flow"),
    };

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Specialize {
                            expr: Box::new(expr(ExprKind::Name("Vec".to_string()))),
                            type_args: vec![type_ref("int32")],
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("Vec[T]() should construct an empty vector")
        ),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: Vec::new(),
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Specialize {
                            expr: Box::new(expr(ExprKind::Name("Set".to_string()))),
                            type_args: vec![type_ref("String")],
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("Set[T]() should construct an empty set")
        ),
        Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: Vec::new(),
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Specialize {
                            expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                            type_args: vec![type_ref("String"), type_ref("int32")],
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("Map[K, V]() should construct an empty map")
        ),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: Vec::new(),
        })
    );

    for (callee, message) in [
        (
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Vec".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            "class `Vec` does not take constructor arguments",
        ),
        (
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Set".to_string()))),
                type_args: vec![type_ref("int32")],
            }),
            "class `Set` does not take constructor arguments",
        ),
        (
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                type_args: vec![type_ref("String"), type_ref("int32")],
            }),
            "class `Map` does not take constructor arguments",
        ),
    ] {
        let error = expect_eval_error(
            interpreter.eval_expr(
                &expr(ExprKind::Call {
                    callee: Box::new(callee),
                    args: vec![positional_arg(expr(ExprKind::Int(1)))],
                }),
                &mut env,
            ),
            "collection constructors reject arguments",
        );
        assert!(error.message.contains(message), "got `{}`", error.message);
    }

    let channel_capacity_error = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Channel".to_string()))),
                    type_args: vec![type_ref("int32")],
                })),
                args: vec![named_arg("capacity", expr(ExprKind::Bool(true)))],
            }),
            &mut env,
        ),
        "Channel[T](capacity=bool) should fail",
    );
    assert!(channel_capacity_error
        .message
        .contains("field `capacity` expects `int32`"));

    match interpreter
        .eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Specialize {
                    expr: Box::new(expr(ExprKind::Name("Channel".to_string()))),
                    type_args: vec![type_ref("int32")],
                })),
                args: vec![named_arg(
                    "capacity",
                    expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                        "err_result".to_string(),
                    ))))),
                )],
            }),
            &mut env,
        )
        .expect("Channel[T](capacity=try ...) should propagate")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from Channel capacity"),
    }

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                        "ok_result".to_string(),
                    ))))),
                    &mut env,
                )
                .expect("try Ok should unwrap")
        ),
        Value::Int(IntegerValue::from_signed(4))
    );
    match interpreter
        .eval_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "err_result".to_string(),
            ))))),
            &mut env,
        )
        .expect("try Err should propagate")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from try"),
    }
    let wrong_try_value = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Int(3))))),
            &mut env,
        ),
        "try on non-Result values should fail",
    );
    assert!(wrong_try_value
        .message
        .contains("requires a `Result` value"));
    let wrong_try_payload = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                "broken_result".to_string(),
            ))))),
            &mut env,
        ),
        "try on malformed Result should fail",
    );
    assert!(wrong_try_payload
        .message
        .contains("invalid `Result` payload"));

    let logical_left_error = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Binary {
                op: BinaryOp::And,
                left: Box::new(expr(ExprKind::Int(1))),
                right: Box::new(expr(ExprKind::Bool(true))),
            }),
            &mut env,
        ),
        "logical operators require bool lhs",
    );
    assert!(logical_left_error
        .message
        .contains("logical operator expects `bool`"));
    let logical_right_error = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Binary {
                op: BinaryOp::Or,
                left: Box::new(expr(ExprKind::Bool(false))),
                right: Box::new(expr(ExprKind::Int(1))),
            }),
            &mut env,
        ),
        "logical operators require bool rhs",
    );
    assert!(logical_right_error
        .message
        .contains("logical operator expects `bool`"));

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("Option".to_string()))),
                        field: "None".to_string(),
                    }),
                    &mut env,
                )
                .expect("Option.None should evaluate"),
        ),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Option".to_string(),
            variant_name: "None".to_string(),
            payload: None,
        })
    );

    let missing_variant = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Status".to_string()))),
                field: "Missing".to_string(),
            }),
            &mut env,
        ),
        "unknown enum variants should fail",
    );
    assert!(missing_variant
        .message
        .contains("enum `Status` has no variant `Missing`"));

    let payload_required = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("Status".to_string()))),
                field: "Done".to_string(),
            }),
            &mut env,
        ),
        "payload variants should reject field-style access",
    );
    assert!(payload_required.message.contains("requires a payload"));

    let bad_index = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Index {
                object: Box::new(expr(ExprKind::Bool(true))),
                index: Box::new(expr(ExprKind::Int(0))),
            }),
            &mut env,
        ),
        "only vectors and maps are indexable",
    );
    assert!(bad_index
        .message
        .contains("cannot index non-vector-or-map value"));

    let sleep_type_error = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("sleep".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Bool(true)))],
            }),
            &mut env,
        ),
        "sleep should reject non-duration arguments",
    );
    assert!(sleep_type_error
        .message
        .contains("`sleep(...)` expects a `Duration`"));

    match interpreter
        .eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("sleep".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                    ExprKind::Name("err_result".to_string()),
                )))))],
            }),
            &mut env,
        )
        .expect("sleep(try Err) should propagate")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from sleep"),
    }

    env.define_typed(
        "min_signed".to_string(),
        Type::named("int128"),
        Value::Int(IntegerValue::from_signed(i128::MIN)),
    );
    let abs_overflow = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("abs".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Name(
                    "min_signed".to_string(),
                )))],
            }),
            &mut env,
        ),
        "abs should reject overflowing signed values",
    );
    assert!(abs_overflow
        .message
        .contains("`abs(...)` overflowed the signed integer range"));

    for (callee_name, args) in [
        (
            "min",
            vec![
                positional_arg(expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                    "err_result".to_string(),
                )))))),
                positional_arg(expr(ExprKind::Int(1))),
            ],
        ),
        (
            "max",
            vec![
                positional_arg(expr(ExprKind::Int(1))),
                positional_arg(expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                    "err_result".to_string(),
                )))))),
            ],
        ),
        (
            "sqrt",
            vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
        ),
        (
            "parse_int32",
            vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
        ),
        (
            "parse_int64",
            vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
        ),
        (
            "parse_float64",
            vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
        ),
    ] {
        match interpreter
            .eval_expr(
                &expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Name(callee_name.to_string()))),
                    args,
                }),
                &mut env,
            )
            .expect("builtin try propagation should preserve Result.Err")
        {
            EvalOutcome::Return(Value::EnumVariant(variant)) => {
                assert_eq!(variant.enum_name, "Result");
                assert_eq!(variant.variant_name, "Err");
            }
            _ => panic!("expected propagated Result.Err from builtin call"),
        }
    }

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("max".to_string()))),
                        args: vec![
                            positional_arg(expr(ExprKind::Float(1.5))),
                            positional_arg(expr(ExprKind::Float(2.5))),
                        ],
                    }),
                    &mut env,
                )
                .expect("max(float, float) should evaluate"),
        ),
        Value::Float(2.5)
    );

    for (expr, message) in [
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("abs".to_string()))),
                args: vec![positional_arg(expr(ExprKind::String("oops".to_string())))],
            }),
            "`abs(...)` expects an integer or float value",
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("sqrt".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Bool(true)))],
            }),
            "`sqrt(...)` expects `float32` or `float64`",
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_int32".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            "`parse_int32(...)` expects `String`",
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_int64".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Bool(true)))],
            }),
            "`parse_int64(...)` expects `String`",
        ),
        (
            expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("parse_float64".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(0)))],
            }),
            "`parse_float64(...)` expects `String`",
        ),
    ] {
        let error = expect_eval_error(
            interpreter.eval_expr(&expr, &mut env),
            "builtin argument type mismatches should fail",
        );
        assert!(error.message.contains(message), "got `{}`", error.message);
    }

    env.define(
        "ghost".to_string(),
        Value::ModuleNamespace(ModuleNamespaceValue {
            path: "missing.path".to_string(),
        }),
    );
    let unknown_namespace = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("ghost".to_string()))),
                field: "child".to_string(),
            }),
            &mut env,
        ),
        "unknown module namespaces should fail on access",
    );
    assert!(unknown_namespace
        .message
        .contains("unknown module namespace `missing.path`"));

    env.define(
        "instance".to_string(),
        Value::Instance(InstanceValue {
            class_name: "Status".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_field = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("instance".to_string()))),
                field: "value".to_string(),
            }),
            &mut env,
        ),
        "missing instance fields should fail",
    );
    assert!(missing_field
        .message
        .contains("class `Status` has no field `value`"));

    let positional_ctor = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("Counter".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            &mut env,
        ),
        "class constructors should reject positional args",
    );
    assert!(positional_ctor
        .message
        .contains("constructor `Counter` requires keyword arguments"));

    let missing_ctor_field = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("Counter".to_string()))),
                args: vec![named_arg("missing", expr(ExprKind::Int(1)))],
            }),
            &mut env,
        ),
        "class constructors should reject unknown fields",
    );
    assert!(missing_ctor_field
        .message
        .contains("class `Counter` has no field named `missing`"));

    let required_ctor_field = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("Counter".to_string()))),
                args: Vec::new(),
            }),
            &mut env,
        ),
        "class constructors should require missing fields",
    );
    assert!(required_ctor_field
        .message
        .contains("missing required field `value` for `Counter`"));

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("Counter".to_string()))),
                        args: vec![named_arg("value", expr(ExprKind::Int(1)))],
                    }),
                    &mut env,
                )
                .expect("constructing Counter with keyword args should work"),
        ),
        Value::Instance(InstanceValue {
            class_name: "Counter".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(1)),
            )]),
        })
    );

    match interpreter
        .eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("Counter".to_string()))),
                args: vec![named_arg(
                    "value",
                    expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                        "err_result".to_string(),
                    ))))),
                )],
            }),
            &mut env,
        )
        .expect("constructor arguments with try Err should propagate")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from constructor"),
    }

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("echo".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::Int(11)))],
                    }),
                    &mut env,
                )
                .expect("direct function calls should evaluate"),
        ),
        Value::Int(IntegerValue::from_signed(11))
    );

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("abs".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::Name(
                            "unsigned_count".to_string(),
                        )))],
                    }),
                    &mut env,
                )
                .expect("abs on unsigned values should be a no-op"),
        ),
        Value::Int(IntegerValue::Unsigned(7))
    );

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("min".to_string()))),
                        args: vec![
                            positional_arg(expr(ExprKind::Int(1))),
                            positional_arg(expr(ExprKind::Int(2))),
                        ],
                    }),
                    &mut env,
                )
                .expect("min on integers should evaluate"),
        ),
        Value::Int(IntegerValue::from_signed(1))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("min".to_string()))),
                        args: vec![
                            positional_arg(expr(ExprKind::Float(1.5))),
                            positional_arg(expr(ExprKind::Float(2.5))),
                        ],
                    }),
                    &mut env,
                )
                .expect("min on floats should evaluate"),
        ),
        Value::Float(1.5)
    );

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("parse_int32".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::String("12".to_string(),)))],
                    }),
                    &mut env,
                )
                .expect("parse_int32 should parse valid text"),
        ),
        result_ok(Value::Int(IntegerValue::from_signed(12)))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("parse_int64".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::String("34".to_string(),)))],
                    }),
                    &mut env,
                )
                .expect("parse_int64 should parse valid text"),
        ),
        result_ok(Value::Int(IntegerValue::from_signed(34)))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("parse_float64".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::String("1.25".to_string(),)))],
                    }),
                    &mut env,
                )
                .expect("parse_float64 should parse valid text"),
        ),
        result_ok(Value::Float(1.25))
    );
}

#[test]
fn interpreter_eval_call_remaining_builtin_trait_and_enum_paths_are_covered() {
    let mut interpreter = test_interpreter(
        "\
trait Factory:
    def make() -> int32

trait Named:
    def label(borrow self) -> String

class Boxed:
    value: int32

impl Factory for Boxed:
    def make() -> int32:
        return 7

impl Named for Boxed:
    def label(borrow self) -> String:
        return \"box\"

enum Status:
    Ready
    Done(int32)

impl Named for Status:
    def label(borrow self) -> String:
        return \"status\"

def main():
    pass
",
    );
    let mut env = Env::with_root();
    env.define(
        "huge".to_string(),
        Value::Int(IntegerValue::from_literal(u128::MAX)),
    );
    env.define(
        "loose_neg".to_string(),
        Value::Int(IntegerValue::from_signed(-4)),
    );
    env.define("loose_float".to_string(), Value::Float(9.0));
    env.define_typed(
        "err_result".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_err(Value::String("boom".to_string())),
    );
    env.define(
        "status".to_string(),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payload: None,
        }),
    );

    let expect_value = |outcome: EvalOutcome| match outcome {
        EvalOutcome::Value(value) => value,
        EvalOutcome::Return(value) => {
            panic!("expected ordinary value flow, got {}", value.render())
        }
    };

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Specialize {
                            expr: Box::new(expr(ExprKind::Name("Vec".to_string()))),
                            type_args: Vec::new(),
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("Vec specialization with no type args should still evaluate"),
        ),
        Value::Vec(VecValue {
            element_type: Type::named("Unknown"),
            elements: Vec::new(),
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Specialize {
                            expr: Box::new(expr(ExprKind::Name("Set".to_string()))),
                            type_args: Vec::new(),
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("Set specialization with no type args should still evaluate"),
        ),
        Value::Set(SetValue {
            element_type: Type::named("Unknown"),
            elements: Vec::new(),
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Specialize {
                            expr: Box::new(expr(ExprKind::Name("Map".to_string()))),
                            type_args: Vec::new(),
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("Map specialization with no type args should still evaluate"),
        ),
        Value::Map(MapValue {
            key_type: Type::named("Unknown"),
            value_type: Type::named("Unknown"),
            entries: Vec::new(),
        })
    );

    for args in [
        vec![named_arg("stop", expr(ExprKind::Name("huge".to_string())))],
        vec![
            named_arg("start", expr(ExprKind::Int(1))),
            named_arg("stop", expr(ExprKind::Name("huge".to_string()))),
        ],
    ] {
        let error = expect_eval_error(
            interpreter.eval_expr(
                &expr(ExprKind::Call {
                    callee: Box::new(expr(ExprKind::Name("range".to_string()))),
                    args,
                }),
                &mut env,
            ),
            "range should reject values outside signed index space",
        );
        assert!(error
            .message
            .contains("`range` arguments must fit in signed index space"));
    }

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("abs".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::Name(
                            "loose_neg".to_string(),
                        )))],
                    }),
                    &mut env,
                )
                .expect("abs should evaluate untyped integers"),
        ),
        Value::Int(IntegerValue::from_signed(4))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("sqrt".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::Name(
                            "loose_float".to_string(),
                        )))],
                    }),
                    &mut env,
                )
                .expect("sqrt should evaluate untyped floats"),
        ),
        Value::Float(3.0)
    );

    match interpreter
        .eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("print".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                    ExprKind::Name("err_result".to_string()),
                )))))],
            }),
            &mut env,
        )
        .expect("print(try Err) should propagate")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from print"),
    }

    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Member {
                            object: Box::new(expr(ExprKind::Name("Boxed".to_string()))),
                            field: "make".to_string(),
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("trait-associated methods should be callable through class names"),
        ),
        Value::Int(IntegerValue::from_signed(7))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Member {
                            object: Box::new(expr(ExprKind::Name("status".to_string()))),
                            field: "label".to_string(),
                        })),
                        args: Vec::new(),
                    }),
                    &mut env,
                )
                .expect("runtime type inference fallback should resolve trait methods"),
        ),
        Value::String("status".to_string())
    );

    match interpreter
        .eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Option".to_string()))),
                    field: "Some".to_string(),
                })),
                args: vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                    ExprKind::Name("err_result".to_string()),
                )))))],
            }),
            &mut env,
        )
        .expect("Option.Some(try Err) should propagate")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from Option.Some"),
    }
    let option_none_payload = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Option".to_string()))),
                    field: "None".to_string(),
                })),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            &mut env,
        ),
        "Option.None should reject payloads",
    );
    assert!(option_none_payload
        .message
        .contains("variant `None` of enum `Option` does not take a payload"));
    let option_some_arity = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Option".to_string()))),
                    field: "Some".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut env,
        ),
        "Option.Some should require one payload",
    );
    assert!(option_some_arity
        .message
        .contains("variant `Some` of enum `Option` expects exactly one payload argument"));

    match interpreter
        .eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Status".to_string()))),
                    field: "Done".to_string(),
                })),
                args: vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                    ExprKind::Name("err_result".to_string()),
                )))))],
            }),
            &mut env,
        )
        .expect("Status.Done(try Err) should propagate")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from Status.Done"),
    }
    let missing_variant = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Status".to_string()))),
                    field: "Missing".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut env,
        ),
        "missing direct enum variants should fail",
    );
    assert!(missing_variant
        .message
        .contains("enum `Status` has no variant `Missing`"));
    let missing_payload = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Status".to_string()))),
                    field: "Done".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut env,
        ),
        "payload variants should require one payload",
    );
    assert!(missing_payload
        .message
        .contains("variant `Done` of enum `Status` expects exactly one payload argument"));
    let extra_payload = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("Status".to_string()))),
                    field: "Ready".to_string(),
                })),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            &mut env,
        ),
        "payload-free variants should reject payloads",
    );
    assert!(extra_payload
        .message
        .contains("variant `Ready` of enum `Status` does not take a payload"));
}

#[test]
fn interpreter_eval_call_member_dispatch_covers_builtin_runtime_and_trait_receivers() {
    let mut interpreter = test_interpreter(
        "\
trait Label:
    def label(borrow self) -> String

class Widget:
    def render(borrow self) -> String:
        return \"widget\"

enum Status:
    Done

impl Label for Status:
    def label(borrow self) -> String:
        return \"done\"

def main():
    pass
",
    );
    let mut env = Env::with_root();
    env.define(
        "number".to_string(),
        Value::Int(IntegerValue::from_signed(7)),
    );
    env.define("ratio".to_string(), Value::Float(4.0));
    env.define("flag".to_string(), Value::Bool(true));
    env.define("text".to_string(), Value::String("Aurora".to_string()));
    env.define(
        "values".to_string(),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define(
        "counts".to_string(),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define(
        "seen".to_string(),
        Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        }),
    );
    env.define("jobs".to_string(), Value::Channel(ChannelValue::new()));
    env.define(
        "task".to_string(),
        Value::Task(TaskValue::from_handle(thread::spawn(|| {
            Ok(Value::Bool(true))
        }))),
    );
    env.define(
        "group".to_string(),
        Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
    );
    env.define(
        "widget".to_string(),
        Value::Instance(InstanceValue {
            class_name: "Widget".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define(
        "status".to_string(),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Done".to_string(),
            payload: None,
        }),
    );
    env.define("unit".to_string(), Value::Unit);

    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("ratio".to_string()))),
                        field: "sqrt".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("float.sqrt() should succeed")
        ),
        Value::Float(2.0)
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("number".to_string()))),
                        field: "to_string".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("int.to_string() should succeed")
        ),
        Value::String("7".to_string())
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("flag".to_string()))),
                        field: "to_string".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("bool.to_string() should succeed")
        ),
        Value::String("true".to_string())
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("text".to_string()))),
                        field: "contains".to_string(),
                    }),
                    &[positional_arg(expr(ExprKind::String("ror".to_string())))],
                    &mut env,
                )
                .expect("string member dispatch should succeed")
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("values".to_string()))),
                        field: "insert".to_string(),
                    }),
                    &[
                        positional_arg(expr(ExprKind::Int(1))),
                        positional_arg(expr(ExprKind::Int(9))),
                    ],
                    &mut env,
                )
                .expect("vec member dispatch should succeed")
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("counts".to_string()))),
                        field: "clear".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("map member dispatch should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("seen".to_string()))),
                        field: "insert".to_string(),
                    }),
                    &[positional_arg(expr(ExprKind::String("go".to_string())))],
                    &mut env,
                )
                .expect("set member dispatch should succeed")
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("jobs".to_string()))),
                        field: "close".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("channel member dispatch should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("task".to_string()))),
                        field: "join".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("task member dispatch should succeed")
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("group".to_string()))),
                        field: "cancel".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("task-group member dispatch should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("widget".to_string()))),
                        field: "render".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("class member dispatch should succeed")
        ),
        Value::String("widget".to_string())
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Name("status".to_string()))),
                        field: "label".to_string(),
                    }),
                    &[],
                    &mut env,
                )
                .expect("runtime type trait dispatch should succeed")
        ),
        Value::String("done".to_string())
    );

    let unsupported = expect_eval_error(
        interpreter.eval_call(
            &expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Name("unit".to_string()))),
                field: "missing".to_string(),
            }),
            &[],
            &mut env,
        ),
        "unsupported members should fail cleanly",
    );
    assert!(unsupported.message.contains("unsupported call target"));
}

#[test]
fn interpreter_collection_method_helpers_cover_vec_map_set_and_string_surface() {
    let mut interpreter = test_interpreter("def main():\n    pass\n");
    let span = Span::new(1, 1);
    let mut env = Env::with_root();
    env.define_typed(
        "values".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "mapping".to_string(),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define_typed(
        "texts".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: vec![
                Value::String("one".to_string()),
                Value::String("two".to_string()),
            ],
        }),
    );
    env.define_typed(
        "names".to_string(),
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ada".to_string())],
        }),
    );
    env.define_typed(
        "err_result".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_err(Value::String("boom".to_string())),
    );

    let expect_value = |outcome: EvalOutcome| match outcome {
        EvalOutcome::Value(value) => value,
        EvalOutcome::Return(value) => panic!("expected value, got {}", value.render()),
    };

    let values_expr = expr(ExprKind::Name("values".to_string()));
    let mapping_expr = expr(ExprKind::Name("mapping".to_string()));
    let names_expr = expr(ExprKind::Name("names".to_string()));

    let vector = match env.get("values").cloned() {
        Some(Value::Vec(vector)) => vector,
        other => panic!("expected vector binding, found {other:?}"),
    };
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(vector.clone(), "len", &values_expr, &[], &mut env, span)
                .expect("vec.len should evaluate"),
        ),
        Value::Int(IntegerValue::from_signed(2))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    vector.clone(),
                    "is_empty",
                    &values_expr,
                    &[],
                    &mut env,
                    span
                )
                .expect("vec.is_empty should evaluate"),
        ),
        Value::Bool(false)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(vector.clone(), "clone", &values_expr, &[], &mut env, span)
                .expect("vec.clone should evaluate"),
        ),
        Value::Vec(vector.clone())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    vector.clone(),
                    "get",
                    &values_expr,
                    &[positional_arg(expr(ExprKind::Int(0)))],
                    &mut env,
                    span,
                )
                .expect("vec.get should evaluate"),
        ),
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    vector.clone(),
                    "contains",
                    &values_expr,
                    &[positional_arg(expr(ExprKind::Int(2)))],
                    &mut env,
                    span,
                )
                .expect("vec.contains should evaluate"),
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    vector.clone(),
                    "push",
                    &values_expr,
                    &[positional_arg(expr(ExprKind::Int(7)))],
                    &mut env,
                    span,
                )
                .expect("vec.push should evaluate"),
        ),
        Value::Unit
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    match env.get("values").cloned() {
                        Some(Value::Vec(vector)) => vector,
                        other => panic!("expected updated vector binding, found {other:?}"),
                    },
                    "pop",
                    &values_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("vec.pop should evaluate"),
        ),
        option_some(Value::Int(IntegerValue::from_signed(7)))
    );
    match interpreter
        .eval_vec_method(
            vector.clone(),
            "push",
            &values_expr,
            &[positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
            &mut env,
            span,
        )
        .expect("vec.push should propagate try Err")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from vec.push"),
    }
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    vector.clone(),
                    "set",
                    &values_expr,
                    &[
                        positional_arg(expr(ExprKind::Int(0))),
                        positional_arg(expr(ExprKind::Int(7))),
                    ],
                    &mut env,
                    span,
                )
                .expect("vec.set should evaluate"),
        ),
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    match env.get("values").cloned() {
                        Some(Value::Vec(vector)) => vector,
                        other => panic!("expected updated vector binding, found {other:?}"),
                    },
                    "remove",
                    &values_expr,
                    &[positional_arg(expr(ExprKind::Int(8)))],
                    &mut env,
                    span,
                )
                .expect("vec.remove should evaluate"),
        ),
        option_none()
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    match env.get("values").cloned() {
                        Some(Value::Vec(vector)) => vector,
                        other => panic!("expected updated vector binding, found {other:?}"),
                    },
                    "swap",
                    &values_expr,
                    &[
                        positional_arg(expr(ExprKind::Int(0))),
                        positional_arg(expr(ExprKind::Int(4))),
                    ],
                    &mut env,
                    span,
                )
                .expect("vec.swap should evaluate"),
        ),
        Value::Bool(false)
    );
    match interpreter
        .eval_vec_method(
            match env.get("values").cloned() {
                Some(Value::Vec(vector)) => vector,
                other => panic!("expected updated vector binding, found {other:?}"),
            },
            "contains",
            &values_expr,
            &[positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
            &mut env,
            span,
        )
        .expect("vec.contains should propagate try Err")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from vec.contains"),
    }
    let extend_error = expect_eval_error(
        interpreter.eval_vec_method(
            match env.get("values").cloned() {
                Some(Value::Vec(vector)) => vector,
                other => panic!("expected updated vector binding, found {other:?}"),
            },
            "extend",
            &values_expr,
            &[positional_arg(expr(ExprKind::Bool(true)))],
            &mut env,
            span,
        ),
        "vec.extend should reject non-vector arguments",
    );
    assert!(extend_error
        .message
        .contains("`extend` requires another `Vec[T]` value"));
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    match env.get("values").cloned() {
                        Some(Value::Vec(vector)) => vector,
                        other => panic!("expected updated vector binding, found {other:?}"),
                    },
                    "insert",
                    &values_expr,
                    &[
                        positional_arg(expr(ExprKind::Int(10))),
                        positional_arg(expr(ExprKind::Int(9))),
                    ],
                    &mut env,
                    span,
                )
                .expect("vec.insert should evaluate"),
        ),
        Value::Bool(false)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    match env.get("values").cloned() {
                        Some(Value::Vec(vector)) => vector,
                        other => panic!("expected updated vector binding, found {other:?}"),
                    },
                    "clear",
                    &values_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("vec.clear should evaluate"),
        ),
        Value::Unit
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_vec_method(
                    match env.get("values").cloned() {
                        Some(Value::Vec(vector)) => vector,
                        other => panic!("expected cleared vector binding, found {other:?}"),
                    },
                    "reverse",
                    &values_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("vec.reverse should evaluate on empty vectors"),
        ),
        Value::Unit
    );
    let bad_vec_method = expect_eval_error(
        interpreter.eval_vec_method(
            match env.get("values").cloned() {
                Some(Value::Vec(vector)) => vector,
                other => panic!("expected vector binding, found {other:?}"),
            },
            "mystery",
            &values_expr,
            &[],
            &mut env,
            span,
        ),
        "unknown vector methods should fail",
    );
    assert!(bad_vec_method
        .message
        .contains("unsupported vector method `mystery`"));

    let map = match env.get("mapping").cloned() {
        Some(Value::Map(map)) => map,
        other => panic!("expected map binding, found {other:?}"),
    };
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(map.clone(), "len", &mapping_expr, &[], &mut env, span)
                .expect("map.len should evaluate"),
        ),
        Value::Int(IntegerValue::from_signed(1))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(map.clone(), "is_empty", &mapping_expr, &[], &mut env, span)
                .expect("map.is_empty should evaluate"),
        ),
        Value::Bool(false)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(map.clone(), "clone", &mapping_expr, &[], &mut env, span)
                .expect("map.clone should evaluate"),
        ),
        Value::Map(map.clone())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    map.clone(),
                    "get",
                    &mapping_expr,
                    &[positional_arg(expr(ExprKind::String("count".to_string())))],
                    &mut env,
                    span,
                )
                .expect("map.get should evaluate"),
        ),
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );
    match interpreter
        .eval_map_method(
            map.clone(),
            "get",
            &mapping_expr,
            &[positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
            &mut env,
            span,
        )
        .expect("map.get should propagate try Err")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from map.get"),
    }
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    map.clone(),
                    "set",
                    &mapping_expr,
                    &[
                        positional_arg(expr(ExprKind::String("next".to_string()))),
                        positional_arg(expr(ExprKind::Int(2))),
                    ],
                    &mut env,
                    span,
                )
                .expect("map.set should insert values"),
        ),
        option_none()
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    match env.get("mapping").cloned() {
                        Some(Value::Map(map)) => map,
                        other => panic!("expected updated map binding, found {other:?}"),
                    },
                    "contains_key",
                    &mapping_expr,
                    &[positional_arg(expr(ExprKind::String("next".to_string())))],
                    &mut env,
                    span,
                )
                .expect("map.contains_key should evaluate"),
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    match env.get("mapping").cloned() {
                        Some(Value::Map(map)) => map,
                        other => panic!("expected updated map binding, found {other:?}"),
                    },
                    "keys",
                    &mapping_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("map.keys should evaluate"),
        ),
        Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: vec![
                Value::String("count".to_string()),
                Value::String("next".to_string()),
            ],
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    match env.get("mapping").cloned() {
                        Some(Value::Map(map)) => map,
                        other => panic!("expected updated map binding, found {other:?}"),
                    },
                    "values",
                    &mapping_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("map.values should evaluate"),
        ),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    match env.get("mapping").cloned() {
                        Some(Value::Map(map)) => map,
                        other => panic!("expected updated map binding, found {other:?}"),
                    },
                    "items",
                    &mapping_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("map.items should evaluate"),
        ),
        Value::Vec(VecValue {
            element_type: Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
            elements: vec![
                Value::Instance(InstanceValue {
                    class_name: "MapEntry".to_string(),
                    fields: BTreeMap::from([
                        ("key".to_string(), Value::String("count".to_string())),
                        (
                            "value".to_string(),
                            Value::Int(IntegerValue::from_signed(1)),
                        ),
                    ]),
                }),
                Value::Instance(InstanceValue {
                    class_name: "MapEntry".to_string(),
                    fields: BTreeMap::from([
                        ("key".to_string(), Value::String("next".to_string())),
                        (
                            "value".to_string(),
                            Value::Int(IntegerValue::from_signed(2)),
                        ),
                    ]),
                }),
            ],
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    match env.get("mapping").cloned() {
                        Some(Value::Map(map)) => map,
                        other => panic!("expected updated map binding, found {other:?}"),
                    },
                    "entries",
                    &mapping_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("map.entries should evaluate"),
        ),
        Value::Vec(VecValue {
            element_type: Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
            elements: vec![
                Value::Instance(InstanceValue {
                    class_name: "MapEntry".to_string(),
                    fields: BTreeMap::from([
                        ("key".to_string(), Value::String("count".to_string())),
                        (
                            "value".to_string(),
                            Value::Int(IntegerValue::from_signed(1)),
                        ),
                    ]),
                }),
                Value::Instance(InstanceValue {
                    class_name: "MapEntry".to_string(),
                    fields: BTreeMap::from([
                        ("key".to_string(), Value::String("next".to_string())),
                        (
                            "value".to_string(),
                            Value::Int(IntegerValue::from_signed(2)),
                        ),
                    ]),
                }),
            ],
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    match env.get("mapping").cloned() {
                        Some(Value::Map(map)) => map,
                        other => panic!("expected updated map binding, found {other:?}"),
                    },
                    "remove",
                    &mapping_expr,
                    &[positional_arg(expr(ExprKind::String("next".to_string())))],
                    &mut env,
                    span,
                )
                .expect("map.remove should evaluate"),
        ),
        option_some(Value::Int(IntegerValue::from_signed(2)))
    );
    let bad_map_extend = expect_eval_error(
        interpreter.eval_map_method(
            match env.get("mapping").cloned() {
                Some(Value::Map(map)) => map,
                other => panic!("expected updated map binding, found {other:?}"),
            },
            "extend",
            &mapping_expr,
            &[positional_arg(expr(ExprKind::Bool(false)))],
            &mut env,
            span,
        ),
        "map.extend should reject non-map arguments",
    );
    assert!(bad_map_extend
        .message
        .contains("`extend` requires another `Map[K, V]` value"));
    assert_eq!(
        expect_value(
            interpreter
                .eval_map_method(
                    match env.get("mapping").cloned() {
                        Some(Value::Map(map)) => map,
                        other => panic!("expected updated map binding, found {other:?}"),
                    },
                    "clear",
                    &mapping_expr,
                    &[],
                    &mut env,
                    span,
                )
                .expect("map.clear should evaluate"),
        ),
        Value::Unit
    );
    let bad_map_method = expect_eval_error(
        interpreter.eval_map_method(
            match env.get("mapping").cloned() {
                Some(Value::Map(map)) => map,
                other => panic!("expected updated map binding, found {other:?}"),
            },
            "missing",
            &mapping_expr,
            &[],
            &mut env,
            span,
        ),
        "unknown map methods should fail",
    );
    assert!(bad_map_method
        .message
        .contains("unsupported map method `missing`"));

    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(" Aurora ".to_string(), "len", &[], &mut env, span)
                .expect("string.len should evaluate"),
        ),
        Value::Int(IntegerValue::from_signed(8))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    "Aurora".to_string(),
                    "contains",
                    &[positional_arg(expr(ExprKind::String("ror".to_string())))],
                    &mut env,
                    span,
                )
                .expect("string.contains should evaluate"),
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    "Aurora".to_string(),
                    "starts_with",
                    &[positional_arg(expr(ExprKind::String("Aur".to_string())))],
                    &mut env,
                    span,
                )
                .expect("string.starts_with should evaluate"),
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    "Aurora".to_string(),
                    "ends_with",
                    &[positional_arg(expr(ExprKind::String("ora".to_string())))],
                    &mut env,
                    span,
                )
                .expect("string.ends_with should evaluate"),
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    "au-ro-ra".to_string(),
                    "split",
                    &[positional_arg(expr(ExprKind::String("-".to_string())))],
                    &mut env,
                    span,
                )
                .expect("string.split should evaluate"),
        ),
        Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: vec![
                Value::String("au".to_string()),
                Value::String("ro".to_string()),
                Value::String("ra".to_string()),
            ],
        })
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    "Aurora".to_string(),
                    "replace",
                    &[
                        positional_arg(expr(ExprKind::String("Aur".to_string()))),
                        positional_arg(expr(ExprKind::String("Our".to_string()))),
                    ],
                    &mut env,
                    span,
                )
                .expect("string.replace should evaluate"),
        ),
        Value::String("Ourora".to_string())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method("AuRoRa".to_string(), "to_lower", &[], &mut env, span)
                .expect("string.to_lower should evaluate"),
        ),
        Value::String("aurora".to_string())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method("AuRoRa".to_string(), "to_upper", &[], &mut env, span)
                .expect("string.to_upper should evaluate"),
        ),
        Value::String("AURORA".to_string())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    "Aurora".to_string(),
                    "strip_prefix",
                    &[positional_arg(expr(ExprKind::String("Aur".to_string())))],
                    &mut env,
                    span,
                )
                .expect("string.strip_prefix should evaluate"),
        ),
        option_some(Value::String("ora".to_string()))
    );
    let contains_error = expect_eval_error(
        interpreter.eval_string_method(
            "Aurora".to_string(),
            "contains",
            &[positional_arg(expr(ExprKind::Int(1)))],
            &mut env,
            span,
        ),
        "string.contains should reject non-string arguments",
    );
    assert!(contains_error
        .message
        .contains("`contains` requires a `String`, found `1`"));
    match interpreter
        .eval_string_method(
            "Aurora".to_string(),
            "starts_with",
            &[positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
            &mut env,
            span,
        )
        .expect("string.starts_with should propagate try Err")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from string.starts_with"),
    }
    let join_type_error = expect_eval_error(
        interpreter.eval_string_method(
            ", ".to_string(),
            "join",
            &[positional_arg(expr(ExprKind::Bool(true)))],
            &mut env,
            span,
        ),
        "string.join should reject non-vectors",
    );
    assert!(join_type_error
        .message
        .contains("`join` requires `Vec[String]`, found `true`"));
    let join_element_error = expect_eval_error(
        interpreter.eval_string_method(
            ", ".to_string(),
            "join",
            &[positional_arg(expr(ExprKind::List(vec![expr(
                ExprKind::Int(1),
            )])))],
            &mut env,
            span,
        ),
        "string.join should reject non-string vector elements",
    );
    assert!(join_element_error
        .message
        .contains("`join` requires `Vec[String]`"));
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method("Aurora".to_string(), "trim", &[], &mut env, span,)
                .expect("string.trim should evaluate"),
        ),
        Value::String("Aurora".to_string())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method("Aurora".to_string(), "clone", &[], &mut env, span,)
                .expect("string.clone should evaluate"),
        ),
        Value::String("Aurora".to_string())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    ", ".to_string(),
                    "join",
                    &[positional_arg(expr(ExprKind::Name("texts".to_string())))],
                    &mut env,
                    span,
                )
                .expect("string.join should evaluate"),
        ),
        Value::String("one, two".to_string())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_string_method(
                    "Aurora".to_string(),
                    "strip_suffix",
                    &[positional_arg(expr(ExprKind::String("ora".to_string())))],
                    &mut env,
                    span,
                )
                .expect("string.strip_suffix should evaluate"),
        ),
        option_some(Value::String("Aur".to_string()))
    );
    let bad_string_method = expect_eval_error(
        interpreter.eval_string_method("Aurora".to_string(), "missing", &[], &mut env, span),
        "unknown string methods should fail",
    );
    assert!(bad_string_method
        .message
        .contains("unsupported string method `missing`"));

    let set = match env.get("names").cloned() {
        Some(Value::Set(set)) => set,
        other => panic!("expected set binding, found {other:?}"),
    };
    assert_eq!(
        expect_value(
            interpreter
                .eval_set_method(set.clone(), "len", &names_expr, &[], &mut env, span)
                .expect("set.len should evaluate"),
        ),
        Value::Int(IntegerValue::from_signed(1))
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_set_method(set.clone(), "is_empty", &names_expr, &[], &mut env, span)
                .expect("set.is_empty should evaluate"),
        ),
        Value::Bool(false)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_set_method(set.clone(), "clone", &names_expr, &[], &mut env, span)
                .expect("set.clone should evaluate"),
        ),
        Value::Set(set.clone())
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_set_method(
                    set.clone(),
                    "contains",
                    &names_expr,
                    &[positional_arg(expr(ExprKind::String("ada".to_string())))],
                    &mut env,
                    span,
                )
                .expect("set.contains should evaluate"),
        ),
        Value::Bool(true)
    );
    assert_eq!(
        expect_value(
            interpreter
                .eval_set_method(
                    set.clone(),
                    "insert",
                    &names_expr,
                    &[positional_arg(expr(ExprKind::String("bob".to_string())))],
                    &mut env,
                    span,
                )
                .expect("set.insert should evaluate"),
        ),
        Value::Bool(true)
    );
    match interpreter
        .eval_set_method(
            set.clone(),
            "insert",
            &names_expr,
            &[positional_arg(expr(ExprKind::Try(Box::new(expr(
                ExprKind::Name("err_result".to_string()),
            )))))],
            &mut env,
            span,
        )
        .expect("set.insert should propagate try Err")
    {
        EvalOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
        }
        _ => panic!("expected propagated Result.Err from set.insert"),
    }
    assert_eq!(
        expect_value(
            interpreter
                .eval_set_method(
                    set.clone(),
                    "remove",
                    &names_expr,
                    &[positional_arg(expr(ExprKind::String(
                        "missing".to_string()
                    )))],
                    &mut env,
                    span,
                )
                .expect("set.remove should evaluate"),
        ),
        Value::Bool(false)
    );
    let bad_set_method = expect_eval_error(
        interpreter.eval_set_method(set, "missing", &names_expr, &[], &mut env, span),
        "unknown set methods should fail",
    );
    assert!(bad_set_method
        .message
        .contains("unsupported set method `missing`"));
}

#[test]
fn interpreter_env_module_and_runtime_type_helpers_cover_additional_branches() {
    let mut interpreter = test_interpreter(
        "\
class Box[T]:
    value: T

enum Status:
    Ready

def helper() -> int32:
    return 1

def main():
    pass
",
    );
    let helper_program = crate::check_source(
        "\
class Widget:
    value: int32

    def build() -> int32:
        return 7

enum Status:
    Value(int32)

def main():
    pass
",
    )
    .expect("helper imported module should type check");

    let mut env = Env::with_root();
    env.define_typed(
        "count".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    assert_eq!(
        env.get("count"),
        Some(&Value::Int(IntegerValue::from_signed(1)))
    );
    assert_eq!(env.get_type("count"), Some(&Type::named("int32")));
    env.push_scope();
    env.define("inner".to_string(), Value::Bool(true));
    assert_eq!(env.get("inner"), Some(&Value::Bool(true)));
    env.set(
        "count".to_string(),
        Value::Int(IntegerValue::from_signed(3)),
    );
    assert_eq!(
        env.get("count"),
        Some(&Value::Int(IntegerValue::from_signed(3)))
    );
    env.pop_scope();
    assert_eq!(env.get("inner"), None);

    let mut widget = helper_program
        .classes
        .get("Widget")
        .expect("helper class should exist")
        .clone();
    widget.module_name = "pkg.tools".to_string();
    let mut status = helper_program
        .enums
        .get("Status")
        .expect("helper enum should exist")
        .clone();
    status.module_name = "pkg.tools".to_string();

    let child_namespace = ModuleNamespace {
        name: "tools".to_string(),
        path: "pkg.tools".to_string(),
        source_path: None,
        functions: BTreeMap::new(),
        all_functions: BTreeMap::new(),
        classes: BTreeMap::from([("Widget".to_string(), widget.clone())]),
        all_classes: BTreeMap::from([("Widget".to_string(), widget)]),
        enums: BTreeMap::from([("Status".to_string(), status.clone())]),
        all_enums: BTreeMap::from([("Status".to_string(), status)]),
        traits: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        modules: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    let parent_namespace = ModuleNamespace {
        name: "pkg".to_string(),
        path: "pkg".to_string(),
        source_path: None,
        functions: BTreeMap::new(),
        all_functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        modules: BTreeMap::from([("tools".to_string(), child_namespace.clone())]),
        imported_modules: BTreeMap::new(),
    };
    assert_eq!(
        Interpreter::find_namespace_in_modules(
            &BTreeMap::from([("pkg".to_string(), parent_namespace.clone())]),
            "pkg.tools"
        )
        .map(|namespace| namespace.path.as_str()),
        Some("pkg.tools")
    );
    assert!(Interpreter::find_namespace_in_modules(&BTreeMap::new(), "missing").is_none());

    interpreter.program = Arc::new(crate::Program {
        module: interpreter.program.module.clone(),
        module_name: "<main>".to_string(),
        source_path: None,
        classes: interpreter.program.classes.clone(),
        enums: interpreter.program.enums.clone(),
        functions: interpreter.program.functions.clone(),
        traits: interpreter.program.traits.clone(),
        trait_impls: interpreter.program.trait_impls.clone(),
        imported_modules: BTreeMap::from([("pkg".to_string(), parent_namespace.clone())]),
        module_registry: BTreeMap::from([
            ("pkg".to_string(), parent_namespace.clone()),
            ("pkg.tools".to_string(), child_namespace.clone()),
        ]),
        top_level_stmts: Vec::new(),
    });

    let mut seeded = Env::with_root();
    interpreter.seed_imported_modules(&mut seeded);
    assert_eq!(
        seeded.get_type("pkg"),
        Some(&Type::Module("pkg".to_string()))
    );
    assert_eq!(
        seeded.get("pkg"),
        Some(&Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg".to_string(),
        }))
    );

    let qualified_widget_build = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                    field: "tools".to_string(),
                })),
                field: "Widget".to_string(),
            })),
            field: "build".to_string(),
        })),
        args: Vec::new(),
    });
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_expr(&qualified_widget_build, &mut seeded)
                .expect("qualified module class methods should evaluate")
        ),
        Value::Int(IntegerValue::from_signed(7))
    );

    let qualified_status_value = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Member {
                object: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                    field: "tools".to_string(),
                })),
                field: "Status".to_string(),
            })),
            field: "Value".to_string(),
        })),
        args: vec![positional_arg(expr(ExprKind::Int(9)))],
    });
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_expr(&qualified_status_value, &mut seeded)
                .expect("qualified enum variant constructors should evaluate")
        ),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Value".to_string(),
            payload: Some(Box::new(Value::Int(IntegerValue::from_signed(9)))),
        })
    );

    let missing_qualified_variant = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Member {
                            object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                            field: "tools".to_string(),
                        })),
                        field: "Status".to_string(),
                    })),
                    field: "Missing".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut seeded,
        ),
        "missing qualified enum variants should fail",
    );
    assert!(missing_qualified_variant
        .message
        .contains("enum `Status` has no variant `Missing`"));

    let qualified_variant_arity = expect_eval_error(
        interpreter.eval_expr(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Member {
                        object: Box::new(expr(ExprKind::Member {
                            object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
                            field: "tools".to_string(),
                        })),
                        field: "Status".to_string(),
                    })),
                    field: "Value".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut seeded,
        ),
        "qualified payload variants should require one argument",
    );
    assert!(qualified_variant_arity
        .message
        .contains("variant `Value` of enum `Status` expects exactly one payload argument"));

    let group_expr = expr(ExprKind::Group(Box::new(expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("pkg".to_string()))),
        field: "tools".to_string(),
    }))));
    assert_eq!(
        interpreter.infer_module_path(&group_expr),
        Some("pkg.tools".to_string())
    );
    let specialized_expr = expr(ExprKind::Specialize {
        expr: Box::new(group_expr.clone()),
        type_args: vec![type_ref("int32")],
    });
    assert_eq!(
        interpreter.qualified_module_item(&expr(ExprKind::Member {
            object: Box::new(specialized_expr),
            field: "Status".to_string(),
        })),
        Some(("pkg.tools".to_string(), "Status".to_string()))
    );

    let none_ref = type_ref("None");
    let str_ref = type_ref("str");
    assert_eq!(Interpreter::lower_runtime_type(&none_ref), Type::Unit);
    assert_eq!(
        Interpreter::lower_runtime_type(&str_ref),
        Type::named("String")
    );
    assert_eq!(
        Interpreter::lower_runtime_type_with_type_params(&type_ref("T"), &["T".to_string()]),
        Type::TypeParam("T".to_string())
    );

    assert_eq!(
        Interpreter::infer_value_type(&Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        })),
        Some(Type::Module("pkg.tools".to_string()))
    );
    assert_eq!(
        Interpreter::infer_value_type(&Value::Unit),
        Some(Type::Unit)
    );

    for (expr, expected) in [
        (
            expr(ExprKind::List(Vec::new())),
            Some(Type::Named("Vec".to_string(), vec![Type::named("Unknown")])),
        ),
        (
            expr(ExprKind::Set(Vec::new())),
            Some(Type::Named("Set".to_string(), vec![Type::named("Unknown")])),
        ),
        (
            expr(ExprKind::Map(Vec::new())),
            Some(Type::Named(
                "Map".to_string(),
                vec![Type::named("Unknown"), Type::named("Unknown")],
            )),
        ),
        (
            expr(ExprKind::Specialize {
                expr: Box::new(expr(ExprKind::Name("Option".to_string()))),
                type_args: vec![type_ref("String")],
            }),
            Some(Type::Named(
                "Option".to_string(),
                vec![Type::named("String")],
            )),
        ),
    ] {
        interpreter.expr_type_cache.borrow_mut().clear();
        assert_eq!(interpreter.infer_expr_type(&expr, &env), expected);
    }

    let mut no_entrypoint = test_interpreter("class Box:\n    value: int32\n");
    let error = no_entrypoint
        .run_main()
        .expect_err("missing entrypoints should fail");
    assert!(error
        .message
        .contains("no `main` function or top-level script statements were found"));
}

#[test]
fn interpreter_select_spawn_call_and_place_helpers_cover_additional_runtime_paths() {
    let mut interpreter = test_interpreter(
        "class Box:\n    value: int32\n\n\
             def add_one(value: int32) -> int32:\n    return value + 1\n\n\
             def default_value(value: int32 = 9) -> int32:\n    return value\n\n\
             def borrowed(value: borrow int32) -> int32:\n    return value\n\n\
             def main():\n    pass\n",
    );
    let span = Span::new(2, 3);
    let mut env = Env::with_root();
    let channel = ChannelValue::new();
    env.define("channel".to_string(), Value::Channel(channel.clone()));
    env.define_typed(
        "delay".to_string(),
        Type::named("Duration"),
        Value::Duration(5),
    );
    env.define_typed(
        "boxed".to_string(),
        Type::named("Box"),
        Value::Instance(InstanceValue {
            class_name: "Box".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(3)),
            )]),
        }),
    );
    env.define_typed(
        "values".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "mapping".to_string(),
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );

    let after_expr = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Name("after".to_string()))),
        args: vec![positional_arg(expr(ExprKind::Name("delay".to_string())))],
    });
    assert!(interpreter
        .prepare_select_deadline(&after_expr, &mut env)
        .expect("after(...) should prepare a deadline")
        .is_some());

    let recv_expr = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Name("channel".to_string()))),
            field: "recv".to_string(),
        })),
        args: Vec::new(),
    });
    assert_eq!(
        interpreter
            .prepare_select_deadline(&recv_expr, &mut env)
            .expect("recv() arms should not prepare deadlines"),
        None
    );

    let bad_after = interpreter
        .prepare_select_deadline(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("after".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            &mut env,
        )
        .expect_err("after(int) should fail");
    assert!(bad_after
        .message
        .contains("`after(...)` expects a `Duration`"));

    let malformed_select = interpreter
        .prepare_select_deadline(&expr(ExprKind::Bool(true)), &mut env)
        .expect_err("non-call select arms should fail");
    assert!(malformed_select
        .message
        .contains("`select` currently supports"));

    let spawned = interpreter
        .eval_spawn(
            false,
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("add_one".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(4)))],
            }),
            &mut env,
            span,
        )
        .expect("spawned task should be created");
    let EvalOutcome::Value(Value::Task(task)) = spawned else {
        panic!("expected spawned task value");
    };
    assert_eq!(
        interpreter
            .join_task(task, span)
            .expect("joined task should return the function result"),
        Value::Int(IntegerValue::from_signed(5))
    );

    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_spawn(
                    true,
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("add_one".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::Int(1)))],
                    }),
                    &mut env,
                    span,
                )
                .expect("detached spawn should succeed")
        ),
        Value::Unit
    );

    let bad_spawn_target = expect_eval_error(
        interpreter.eval_spawn(false, &expr(ExprKind::Bool(true)), &mut env, span),
        "spawn requires a call expression",
    );
    assert!(bad_spawn_target
        .message
        .contains("requires a function or method call expression"));

    let method_spawn = expect_eval_error(
        interpreter.eval_spawn(
            false,
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("channel".to_string()))),
                    field: "recv".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut env,
            span,
        ),
        "spawn should reject non-name callees",
    );
    assert!(method_spawn.message.contains("named function calls only"));

    let borrowed_spawn = expect_eval_error(
        interpreter.eval_spawn(
            false,
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("borrowed".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Int(3)))],
            }),
            &mut env,
            span,
        ),
        "spawn should reject borrowed params",
    );
    assert!(borrowed_spawn
        .message
        .contains("does not yet support borrowed parameter"));

    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_call(
                    &expr(ExprKind::Specialize {
                        expr: Box::new(expr(ExprKind::Name("default_value".to_string()))),
                        type_args: vec![type_ref("int32")],
                    }),
                    &[],
                    &mut env,
                )
                .expect("specialized call should fall through to the function body")
        ),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        expect_value_outcome(
            interpreter
                .eval_expr(
                    &expr(ExprKind::Call {
                        callee: Box::new(expr(ExprKind::Name("add_one".to_string()))),
                        args: vec![positional_arg(expr(ExprKind::Int(7)))],
                    }),
                    &mut env,
                )
                .expect("eval_expr should cover ordinary call dispatch")
        ),
        Value::Int(IntegerValue::from_signed(8))
    );

    let boxed_member = expr(ExprKind::Member {
        object: Box::new(expr(ExprKind::Name("boxed".to_string()))),
        field: "value".to_string(),
    });
    assert_eq!(
        interpreter
            .read_assign_target(
                &AssignTarget::Member {
                    object: Box::new(expr(ExprKind::Name("boxed".to_string()))),
                    field: "value".to_string(),
                },
                &mut env,
                span,
            )
            .expect("member assign target should read successfully"),
        Value::Int(IntegerValue::from_signed(3))
    );
    interpreter
        .write_place_expr(
            &boxed_member,
            &mut env,
            Value::Int(IntegerValue::from_signed(11)),
        )
        .expect("member place write should succeed");

    let vec_index = expr(ExprKind::Index {
        object: Box::new(expr(ExprKind::Group(Box::new(expr(ExprKind::Name(
            "values".to_string(),
        )))))),
        index: Box::new(expr(ExprKind::Int(1))),
    });
    assert_eq!(
        interpreter
            .read_assign_target(
                &AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("values".to_string()))),
                    index: Box::new(expr(ExprKind::Int(0))),
                },
                &mut env,
                span,
            )
            .expect("vector assign target should read successfully"),
        Value::Int(IntegerValue::from_signed(1))
    );
    interpreter
        .write_place_expr(
            &vec_index,
            &mut env,
            Value::Int(IntegerValue::from_signed(22)),
        )
        .expect("vector place write should succeed");

    let map_index = expr(ExprKind::Index {
        object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
        index: Box::new(expr(ExprKind::String("count".to_string()))),
    });
    assert_eq!(
        interpreter
            .read_assign_target(
                &AssignTarget::Index {
                    object: Box::new(expr(ExprKind::Name("mapping".to_string()))),
                    index: Box::new(expr(ExprKind::String("count".to_string()))),
                },
                &mut env,
                span,
            )
            .expect("map assign target should read successfully"),
        Value::Int(IntegerValue::from_signed(1))
    );
    interpreter
        .write_place_expr(
            &map_index,
            &mut env,
            Value::Int(IntegerValue::from_signed(7)),
        )
        .expect("map place write should succeed");

    match env.get("boxed") {
        Some(Value::Instance(instance)) => {
            assert_eq!(
                instance.fields.get("value"),
                Some(&Value::Int(IntegerValue::from_signed(11)))
            );
        }
        other => panic!("expected updated boxed instance, found {other:?}"),
    }
    match env.get("values") {
        Some(Value::Vec(vector)) => {
            assert_eq!(
                vector.elements,
                vec![
                    Value::Int(IntegerValue::from_signed(1)),
                    Value::Int(IntegerValue::from_signed(22)),
                ]
            );
        }
        other => panic!("expected updated vector, found {other:?}"),
    }
    match env.get("mapping") {
        Some(Value::Map(map)) => {
            assert_eq!(
                map.entries,
                vec![(
                    Value::String("count".to_string()),
                    Value::Int(IntegerValue::from_signed(7)),
                )]
            );
        }
        other => panic!("expected updated map, found {other:?}"),
    }
}

#[test]
fn interpreter_join_task_reports_cached_and_missing_handle_failures() {
    let mut interpreter = test_interpreter("def main():\n    pass\n");
    let span = Span::new(1, 1);
    let cached_error = TaskValue {
        inner: Arc::new(TaskState {
            handle: Mutex::new(TaskHandle::Completed(Err("cached failure".to_string()))),
        }),
    };
    let cached = interpreter
        .join_task(cached_error, span)
        .expect_err("cached task errors should surface immediately");
    assert!(cached.message.contains("cached failure"));

    let missing_handle = TaskValue {
        inner: Arc::new(TaskState {
            handle: Mutex::new(TaskHandle::Running(None)),
        }),
    };
    let missing = interpreter
        .join_task(missing_handle, span)
        .expect_err("missing join handles should be reported");
    assert!(missing
        .message
        .contains("task join handle was not available"));
}

#[test]
fn interpreter_exec_stmt_loop_with_and_while_paths_cover_additional_branches() {
    let mut interpreter = test_interpreter(
            "class Resource:\n    closed: bool = false\n\n    def close(borrow mut self):\n        self.closed = true\n\ndef main():\n    pass\n",
        );
    let span = Span::new(4, 2);
    let mut env = Env::with_root();
    env.define_typed(
        "values".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "shared_values".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(5))],
        }),
    );
    env.define_typed(
        "items".to_string(),
        Type::Named("Set".to_string(), vec![Type::named("int32")]),
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(3)),
                Value::Int(IntegerValue::from_signed(4)),
            ],
        }),
    );
    env.define_typed(
        "resource".to_string(),
        Type::named("Resource"),
        Value::Instance(InstanceValue {
            class_name: "Resource".to_string(),
            fields: BTreeMap::from([("closed".to_string(), Value::Bool(false))]),
        }),
    );

    let borrow_mut_loop = Stmt::For(ForStmt {
        binding: "value".to_string(),
        iterable: expr(ExprKind::Name("values".to_string())),
        borrow_mode: Some(ReceiverKind::BorrowMut),
        body: vec![Stmt::If(IfStmt {
            branches: vec![IfBranch {
                condition: expr(ExprKind::Binary {
                    op: BinaryOp::Eq,
                    left: Box::new(expr(ExprKind::Name("value".to_string()))),
                    right: Box::new(expr(ExprKind::Int(1))),
                }),
                body: vec![
                    Stmt::Assign(AssignStmt {
                        mutable: false,
                        target: AssignTarget::Name("value".to_string()),
                        annotation: None,
                        op: Some(BinaryOp::Add),
                        value: expr(ExprKind::Int(10)),
                        span,
                    }),
                    Stmt::Continue(ContinueStmt { span }),
                ],
                span,
            }],
            else_body: Some(vec![Stmt::Return(ReturnStmt {
                value: Some(expr(ExprKind::Name("value".to_string()))),
                span,
            })]),
            span,
        })],
        span,
    });

    match interpreter
        .exec_stmt(&borrow_mut_loop, &mut env)
        .expect("borrow-mut vec loop should execute")
    {
        ExecFlow::Return(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(2));
        }
        ExecFlow::Return(other) => {
            panic!("expected loop to return second int element, found {other:?}")
        }
        ExecFlow::Continue => panic!("expected loop to return, found continue"),
        ExecFlow::Break => panic!("expected loop to return, found break"),
        ExecFlow::ContinueLoop => panic!("expected loop to return, found continue-loop"),
    }
    match env.get("values") {
        Some(Value::Vec(vector)) => {
            assert_eq!(
                vector.elements,
                vec![
                    Value::Int(IntegerValue::from_signed(11)),
                    Value::Int(IntegerValue::from_signed(2)),
                ]
            );
        }
        other => panic!("expected updated borrow-mut vector, found {other:?}"),
    }

    let shared_break_loop = Stmt::For(ForStmt {
        binding: "item".to_string(),
        iterable: expr(ExprKind::Name("shared_values".to_string())),
        borrow_mode: Some(ReceiverKind::Borrow),
        body: vec![Stmt::Break(BreakStmt { span })],
        span,
    });
    assert!(matches!(
        interpreter
            .exec_stmt(&shared_break_loop, &mut env)
            .expect("shared vec loop should execute"),
        ExecFlow::Continue
    ));

    let set_break_loop = Stmt::For(ForStmt {
        binding: "item".to_string(),
        iterable: expr(ExprKind::Name("items".to_string())),
        borrow_mode: Some(ReceiverKind::Borrow),
        body: vec![Stmt::Break(BreakStmt { span })],
        span,
    });
    assert!(matches!(
        interpreter
            .exec_stmt(&set_break_loop, &mut env)
            .expect("set loop should execute"),
        ExecFlow::Continue
    ));

    let unsupported_loop = Stmt::For(ForStmt {
        binding: "item".to_string(),
        iterable: expr(ExprKind::Bool(true)),
        borrow_mode: None,
        body: vec![Stmt::Pass(PassStmt { span })],
        span,
    });
    let unsupported = match interpreter.exec_stmt(&unsupported_loop, &mut env) {
        Ok(_) => panic!("unsupported iterables should fail"),
        Err(error) => error,
    };
    assert!(unsupported.message.contains(
        "`for` currently requires a `Range`, `Channel[T]`, `Vec[T]`, or `Set[T]` iterable"
    ));

    let with_stmt = Stmt::With(WithStmt {
        binding: "handle".to_string(),
        value: expr(ExprKind::Name("resource".to_string())),
        body: vec![Stmt::Pass(PassStmt { span })],
        span,
    });
    assert!(matches!(
        interpreter
            .exec_stmt(&with_stmt, &mut env)
            .expect("with statement should execute"),
        ExecFlow::Continue
    ));

    let while_error = match interpreter.exec_stmt(
        &Stmt::While(WhileStmt {
            condition: expr(ExprKind::Int(1)),
            body: vec![Stmt::Pass(PassStmt { span })],
            span,
        }),
        &mut env,
    ) {
        Ok(_) => panic!("non-bool while conditions should fail"),
        Err(error) => error,
    };
    assert!(while_error
        .message
        .contains("`while` condition must evaluate to `bool`"));
}

#[test]
fn interpreter_exec_stmt_try_propagation_and_match_paths_cover_remaining_edges() {
    let mut interpreter =
        test_interpreter("enum Status:\n    Ready\n    Done(int32)\n\ndef main():\n    pass\n");
    let span = Span::new(1, 1);
    let mut env = Env::with_root();
    env.define_typed(
        "err_result".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_err(Value::String("boom".to_string())),
    );
    env.define_typed(
        "status".to_string(),
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Done".to_string(),
            payload: Some(Box::new(Value::Int(IntegerValue::from_signed(9)))),
        }),
    );
    env.define_typed(
        "counter".to_string(),
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(4)),
    );

    let add_assign = Stmt::Assign(AssignStmt {
        mutable: false,
        target: AssignTarget::Name("counter".to_string()),
        annotation: None,
        op: Some(BinaryOp::Add),
        value: expr(ExprKind::Int(3)),
        span,
    });
    assert!(matches!(
        interpreter
            .exec_stmt(&add_assign, &mut env)
            .expect("compound assignment should execute"),
        ExecFlow::Continue
    ));
    assert_eq!(
        env.get("counter"),
        Some(&Value::Int(IntegerValue::from_signed(7)))
    );

    let annotated_binding = Stmt::Assign(AssignStmt {
        mutable: false,
        target: AssignTarget::Name("widened".to_string()),
        annotation: Some(type_ref("int64")),
        op: None,
        value: expr(ExprKind::Int(5)),
        span,
    });
    assert!(matches!(
        interpreter
            .exec_stmt(&annotated_binding, &mut env)
            .expect("new annotated bindings should execute"),
        ExecFlow::Continue
    ));
    assert_eq!(
        env.get("widened"),
        Some(&Value::Int(IntegerValue::from_signed(5)))
    );

    let try_name = || {
        expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
            "err_result".to_string(),
        )))))
    };
    let expect_try_return = |flow: ExecFlow| match flow {
        ExecFlow::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
            match variant.payload.as_deref() {
                Some(Value::String(text)) => assert_eq!(text, "boom"),
                other => panic!("expected Err payload string, found {other:?}"),
            }
        }
        ExecFlow::Return(other) => panic!("expected propagated string error, found {other:?}"),
        ExecFlow::Continue => panic!("expected propagated return, found continue"),
        ExecFlow::Break => panic!("expected propagated return, found break"),
        ExecFlow::ContinueLoop => panic!("expected propagated return, found continue-loop"),
    };

    expect_try_return(
        interpreter
            .exec_stmt(
                &Stmt::If(IfStmt {
                    branches: vec![IfBranch {
                        condition: try_name(),
                        body: Vec::new(),
                        span,
                    }],
                    else_body: None,
                    span,
                }),
                &mut env,
            )
            .expect("try in if conditions should propagate"),
    );
    expect_try_return(
        interpreter
            .exec_stmt(
                &Stmt::Match(MatchStmt {
                    scrutinee: try_name(),
                    borrow_mode: None,
                    arms: vec![MatchArm {
                        pattern: Pattern::Wildcard(span),
                        body: Vec::new(),
                        span,
                    }],
                    span,
                }),
                &mut env,
            )
            .expect("try in match scrutinees should propagate"),
    );
    expect_try_return(
        interpreter
            .exec_stmt(
                &Stmt::For(ForStmt {
                    binding: "value".to_string(),
                    iterable: try_name(),
                    borrow_mode: None,
                    body: Vec::new(),
                    span,
                }),
                &mut env,
            )
            .expect("try in for iterables should propagate"),
    );
    expect_try_return(
        interpreter
            .exec_stmt(
                &Stmt::With(WithStmt {
                    binding: "resource".to_string(),
                    value: try_name(),
                    body: Vec::new(),
                    span,
                }),
                &mut env,
            )
            .expect("try in with values should propagate"),
    );
    expect_try_return(
        interpreter
            .exec_stmt(
                &Stmt::While(WhileStmt {
                    condition: try_name(),
                    body: Vec::new(),
                    span,
                }),
                &mut env,
            )
            .expect("try in while conditions should propagate"),
    );
    expect_try_return(
        interpreter
            .exec_stmt(
                &Stmt::Expr(ExprStmt {
                    expr: try_name(),
                    span,
                }),
                &mut env,
            )
            .expect("try in expression statements should propagate"),
    );

    match interpreter
        .exec_stmt(
            &Stmt::Match(MatchStmt {
                scrutinee: expr(ExprKind::Name("status".to_string())),
                borrow_mode: None,
                arms: vec![MatchArm {
                    pattern: Pattern::Variant(VariantPattern {
                        enum_name: Some("Status".to_string()),
                        variant_name: "Done".to_string(),
                        binding: Some("payload".to_string()),
                        span,
                    }),
                    body: vec![Stmt::Return(ReturnStmt {
                        value: Some(expr(ExprKind::Name("payload".to_string()))),
                        span,
                    })],
                    span,
                }],
                span,
            }),
            &mut env,
        )
        .expect("payload matches should bind and return")
    {
        ExecFlow::Return(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(9));
        }
        ExecFlow::Return(other) => {
            panic!("expected bound payload return, found {other:?}");
        }
        ExecFlow::Continue => panic!("expected return from match arm"),
        ExecFlow::Break => panic!("expected return from match arm"),
        ExecFlow::ContinueLoop => panic!("expected return from match arm"),
    }

    let no_arm_error = match interpreter.exec_stmt(
        &Stmt::Match(MatchStmt {
            scrutinee: expr(ExprKind::Name("status".to_string())),
            borrow_mode: None,
            arms: vec![MatchArm {
                pattern: Pattern::Variant(VariantPattern {
                    enum_name: Some("Status".to_string()),
                    variant_name: "Ready".to_string(),
                    binding: None,
                    span,
                }),
                body: Vec::new(),
                span,
            }],
            span,
        }),
        &mut env,
    ) {
        Ok(_) => panic!("unmatched runtime matches should fail"),
        Err(error) => error,
    };
    assert!(no_arm_error
        .message
        .contains("no `match` arm matched the scrutinee at runtime"));
}

#[test]
fn interpreter_additional_loop_select_and_eval_expr_edges_cover_remaining_paths() {
    let mut interpreter = test_interpreter(
            "class Resource:\n    closed: bool = false\n\n    def close(borrow mut self):\n        self.closed = true\n\ndef main():\n    pass\n",
        );
    let span = Span::new(1, 1);
    let mut env = Env::with_root();
    env.define_typed(
        "err_result".to_string(),
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_err(Value::String("boom".to_string())),
    );
    env.define_typed(
        "resource".to_string(),
        Type::named("Resource"),
        Value::Instance(InstanceValue {
            class_name: "Resource".to_string(),
            fields: BTreeMap::from([("closed".to_string(), Value::Bool(false))]),
        }),
    );
    env.define_typed(
        "zero".to_string(),
        Type::named("Duration"),
        Value::Duration(0),
    );
    env.define_typed(
        "negative".to_string(),
        Type::named("Duration"),
        Value::Duration(-1),
    );

    let channel = ChannelValue::new();
    let _ = channel.send(Value::Int(IntegerValue::from_signed(9)));
    channel.close();
    env.define("jobs".to_string(), Value::Channel(channel.clone()));

    env.define(
        "owned_values".to_string(),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(7)),
                Value::Int(IntegerValue::from_signed(8)),
            ],
        }),
    );
    env.define(
        "shared_values".to_string(),
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(5))],
        }),
    );
    env.define(
        "items".to_string(),
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(4))],
        }),
    );

    let range_loop = Stmt::For(ForStmt {
        binding: "value".to_string(),
        iterable: expr(ExprKind::Call {
            callee: Box::new(expr(ExprKind::Name("range".to_string()))),
            args: vec![
                positional_arg(expr(ExprKind::Int(0))),
                positional_arg(expr(ExprKind::Int(3))),
            ],
        }),
        borrow_mode: None,
        body: vec![Stmt::Return(ReturnStmt {
            value: Some(expr(ExprKind::Name("value".to_string()))),
            span,
        })],
        span,
    });
    match interpreter
        .exec_stmt(&range_loop, &mut env)
        .expect("range loop should execute")
    {
        ExecFlow::Return(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(0));
        }
        _ => panic!("expected range loop to return its first item"),
    }

    let channel_loop = Stmt::For(ForStmt {
        binding: "job".to_string(),
        iterable: expr(ExprKind::Name("jobs".to_string())),
        borrow_mode: None,
        body: vec![Stmt::Return(ReturnStmt {
            value: Some(expr(ExprKind::Name("job".to_string()))),
            span,
        })],
        span,
    });
    match interpreter
        .exec_stmt(&channel_loop, &mut env)
        .expect("channel loop should execute")
    {
        ExecFlow::Return(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(9));
        }
        _ => panic!("expected channel loop to return its first item"),
    }

    let shared_loop = Stmt::For(ForStmt {
        binding: "item".to_string(),
        iterable: expr(ExprKind::Name("shared_values".to_string())),
        borrow_mode: Some(ReceiverKind::Borrow),
        body: vec![Stmt::Return(ReturnStmt {
            value: Some(expr(ExprKind::Name("item".to_string()))),
            span,
        })],
        span,
    });
    match interpreter
        .exec_stmt(&shared_loop, &mut env)
        .expect("shared vec loop should execute")
    {
        ExecFlow::Return(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(5));
        }
        _ => panic!("expected shared vec loop to return its first item"),
    }

    let owned_loop = Stmt::For(ForStmt {
        binding: "item".to_string(),
        iterable: expr(ExprKind::Name("owned_values".to_string())),
        borrow_mode: None,
        body: vec![Stmt::Return(ReturnStmt {
            value: Some(expr(ExprKind::Name("item".to_string()))),
            span,
        })],
        span,
    });
    match interpreter
        .exec_stmt(&owned_loop, &mut env)
        .expect("owned vec loop should execute")
    {
        ExecFlow::Return(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(7));
        }
        _ => panic!("expected owned vec loop to return its first item"),
    }

    let set_loop = Stmt::For(ForStmt {
        binding: "item".to_string(),
        iterable: expr(ExprKind::Name("items".to_string())),
        borrow_mode: Some(ReceiverKind::Borrow),
        body: vec![Stmt::Return(ReturnStmt {
            value: Some(expr(ExprKind::Name("item".to_string()))),
            span,
        })],
        span,
    });
    match interpreter
        .exec_stmt(&set_loop, &mut env)
        .expect("set loop should execute")
    {
        ExecFlow::Return(Value::Int(value)) => {
            assert_eq!(value, IntegerValue::from_signed(4));
        }
        _ => panic!("expected set loop to return its first item"),
    }

    assert!(matches!(
        interpreter
            .exec_stmt(
                &Stmt::While(WhileStmt {
                    condition: expr(ExprKind::Bool(false)),
                    body: vec![Stmt::Pass(PassStmt { span })],
                    span,
                }),
                &mut env,
            )
            .expect("while false should finish immediately"),
        ExecFlow::Continue
    ));

    let after_expr = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Name("after".to_string()))),
        args: vec![positional_arg(expr(ExprKind::Name("zero".to_string())))],
    });
    match interpreter
        .exec_select(
            &[SelectArm {
                binding: Some("tick".to_string()),
                expr: after_expr.clone(),
                body: vec![Stmt::Return(ReturnStmt {
                    value: Some(expr(ExprKind::Name("tick".to_string()))),
                    span,
                })],
                span,
            }],
            &mut env,
        )
        .expect("after arm should fire immediately")
    {
        ExecFlow::Return(Value::Unit) => {}
        _ => panic!("expected select after arm to bind unit"),
    }

    let timer_return = interpreter
        .prepare_select_deadline(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("after".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                    ExprKind::Name("err_result".to_string()),
                )))))],
            }),
            &mut env,
        )
        .expect_err("timer setup should reject propagated returns");
    assert!(timer_return
        .message
        .contains("timer preparation cannot return early"));

    let timer_overflow = interpreter
        .prepare_select_deadline(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Name("after".to_string()))),
                args: vec![positional_arg(expr(ExprKind::Name("negative".to_string())))],
            }),
            &mut env,
        )
        .expect_err("negative durations should be rejected");
    assert!(timer_overflow
        .message
        .contains("does not fit in the runtime timer range"));

    let recv_expr = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Name("jobs".to_string()))),
            field: "recv".to_string(),
        })),
        args: Vec::new(),
    });
    assert_eq!(
        interpreter
            .try_select_arm(&recv_expr, &mut env, None, true)
            .expect("closed recv arms can be ignored with timers"),
        None
    );
    assert_eq!(
        interpreter
            .try_select_arm(&recv_expr, &mut env, None, false)
            .expect("closed recv arms should surface Option.None without timers"),
        Some(option_none())
    );

    let recv_return = interpreter
        .try_select_arm(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                        "err_result".to_string(),
                    )))))),
                    field: "recv".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut env,
            None,
            false,
        )
        .expect_err("recv arms should reject early returns");
    assert!(recv_return
        .message
        .contains("receive arm cannot return early"));

    let recv_receiver = interpreter
        .try_select_arm(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Bool(true))),
                    field: "recv".to_string(),
                })),
                args: Vec::new(),
            }),
            &mut env,
            None,
            false,
        )
        .expect_err("recv arms require channels");
    assert!(recv_receiver
        .message
        .contains("receive arms require a channel receiver"));

    let closed_send = ChannelValue::new();
    closed_send.close();
    env.define("closed_jobs".to_string(), Value::Channel(closed_send));
    let send_expr = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Member {
            object: Box::new(expr(ExprKind::Name("closed_jobs".to_string()))),
            field: "send".to_string(),
        })),
        args: vec![positional_arg(expr(ExprKind::Int(7)))],
    });
    assert_eq!(
        interpreter
            .try_select_arm(&send_expr, &mut env, None, false)
            .expect("closed send arms should surface SendError"),
        Some(result_err(send_error_closed(Value::Int(
            IntegerValue::from_signed(7),
        ))))
    );

    let send_return = interpreter
        .try_select_arm(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
                        "err_result".to_string(),
                    )))))),
                    field: "send".to_string(),
                })),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            &mut env,
            None,
            false,
        )
        .expect_err("send receiver evaluation should reject early returns");
    assert!(send_return.message.contains("send arm cannot return early"));

    let send_value_return = interpreter
        .try_select_arm(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Name("jobs".to_string()))),
                    field: "send".to_string(),
                })),
                args: vec![positional_arg(expr(ExprKind::Try(Box::new(expr(
                    ExprKind::Name("err_result".to_string()),
                )))))],
            }),
            &mut env,
            None,
            false,
        )
        .expect_err("send value evaluation should reject early returns");
    assert!(send_value_return
        .message
        .contains("send arm cannot return early"));

    let send_receiver = interpreter
        .try_select_arm(
            &expr(ExprKind::Call {
                callee: Box::new(expr(ExprKind::Member {
                    object: Box::new(expr(ExprKind::Bool(true))),
                    field: "send".to_string(),
                })),
                args: vec![positional_arg(expr(ExprKind::Int(1)))],
            }),
            &mut env,
            None,
            false,
        )
        .expect_err("send arms require channels");
    assert!(send_receiver
        .message
        .contains("send arms require a channel receiver"));

    let malformed_select = interpreter
        .try_select_arm(&expr(ExprKind::Bool(true)), &mut env, None, false)
        .expect_err("non-call select arms should fail");
    assert!(malformed_select
        .message
        .contains("`select` currently supports"));

    let after_not_prepared = interpreter
        .try_select_arm(&after_expr, &mut env, None, false)
        .expect_err("after arms require prepared deadlines");
    assert!(after_not_prepared
        .message
        .contains("timer was not prepared correctly"));

    let try_name = || {
        expr(ExprKind::Try(Box::new(expr(ExprKind::Name(
            "err_result".to_string(),
        )))))
    };
    for collection_expr in [
        expr(ExprKind::List(vec![try_name()])),
        expr(ExprKind::Set(vec![try_name()])),
        expr(ExprKind::Map(vec![MapEntryExpr {
            key: expr(ExprKind::String("answer".to_string())),
            value: try_name(),
        }])),
        expr(ExprKind::FString(vec![FormatPart::Expr(try_name())])),
        expr(ExprKind::Cast {
            expr: Box::new(try_name()),
            ty: type_ref("int32"),
        }),
        expr(ExprKind::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(try_name()),
        }),
    ] {
        match interpreter
            .eval_expr(&collection_expr, &mut env)
            .expect("embedded try should propagate through expression helpers")
        {
            EvalOutcome::Return(Value::EnumVariant(variant)) => {
                assert_eq!(variant.enum_name, "Result");
                assert_eq!(variant.variant_name, "Err");
            }
            _ => panic!("expected propagated return from collection expr"),
        }
    }
}
