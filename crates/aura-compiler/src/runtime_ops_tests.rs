use super::{
    builtin_function_for_name, builtin_member_for_receiver, ArrayOp, DictOp, FunctionOp,
    HostBuiltin, ListOp, PrintKind, QueueOp, ResourceOp, RngOp, RuntimeOp, RuntimeTarget,
    ScalarElement, ScalarOp, SetOp, SpanOp, StrOp, TaskGroupOp, TaskOp, ValueOp,
};
use crate::mir::{specialized_member_op, MirArg, Operand};
use crate::native_runtime::HOST_BUILTIN_SYMBOLS;
use crate::runtime_abi::RuntimeFn;
use crate::sema::{optional_type, Type};

fn named(name: &str, args: Vec<Type>) -> Type {
    Type::Named(name.to_string(), args)
}

fn result(ok: Type, error: &str) -> Type {
    named("Result", vec![ok, Type::named(error)])
}

fn bytes() -> Type {
    named("list", vec![Type::named("uint8")])
}

/// A receiver type each op applies to.
fn sample_receiver(op: RuntimeOp) -> Type {
    match op {
        RuntimeOp::Resource(op) => Type::named(op.receiver_type_name()),
        RuntimeOp::Rng(_) => Type::named("random.Rng"),
        RuntimeOp::Scalar(ScalarOp::Sqrt) => Type::named("float64"),
        RuntimeOp::Scalar(ScalarOp::DurationToMilliseconds | ScalarOp::DurationToSeconds) => {
            Type::named("Duration")
        }
        RuntimeOp::Scalar(_) | RuntimeOp::Value(ValueOp::ToString) => Type::named("int32"),
        RuntimeOp::Value(ValueOp::Clone) => Type::named("str"),
        RuntimeOp::Value(ValueOp::Copy) => named("list", vec![Type::named("int64")]),
        RuntimeOp::Str(_) => Type::named("str"),
        RuntimeOp::Queue(_) => named("Queue", vec![Type::named("int64")]),
        RuntimeOp::Task(_) => named("Task", vec![Type::named("str")]),
        RuntimeOp::TaskGroup(_) => Type::named("TaskGroup"),
        RuntimeOp::List(
            ListOp::PushScalar(kind)
            | ListOp::IndexScalar(kind)
            | ListOp::IndexScalarUnchecked(kind),
        ) => named("list", vec![scalar_element_type(kind)]),
        RuntimeOp::List(_) | RuntimeOp::Span(_) => named("list", vec![Type::named("str")]),
        RuntimeOp::Dict(_) => named("dict", vec![Type::named("str"), Type::named("int64")]),
        RuntimeOp::Set(SetOp::ContainsInt64) => named("set", vec![Type::named("int64")]),
        RuntimeOp::Set(_) => named("set", vec![Type::named("str")]),
        RuntimeOp::Array(_) => named("Array", vec![Type::named("float64")]),
        RuntimeOp::Function(_) | RuntimeOp::Print(_) | RuntimeOp::Host(_) => {
            unreachable!("{op} has no receiver")
        }
    }
}

fn scalar_element_type(kind: ScalarElement) -> Type {
    Type::named(match kind {
        ScalarElement::Int64 => "int64",
        ScalarElement::Float64 => "float64",
        ScalarElement::Bool => "bool",
    })
}

/// The argument types each op's result type reads: `Array.map` names its
/// result dtype by its callback's result.
fn sample_args(op: RuntimeOp) -> Vec<Option<Type>> {
    match op {
        RuntimeOp::Array(ArrayOp::Map) => vec![Some(Type::Function {
            params: Vec::new(),
            return_type: Box::new(Type::named("int64")),
        })],
        _ => Vec::new(),
    }
}

fn positional(value: Operand) -> MirArg {
    MirArg {
        name: None,
        value,
        writeback_place: None,
    }
}

#[test]
fn every_runtime_op_resolves_from_its_receiver_and_has_a_result_type() {
    for op in RuntimeOp::all().filter(|op| op.has_receiver()) {
        let receiver = sample_receiver(op);
        let Type::Named(name, _) = &receiver else {
            unreachable!("sample receivers are named types")
        };
        assert!(op.applies_to(name), "{op} applies to `{receiver}`");
        assert!(!op.applies_to("Range"), "{op} does not apply to `Range`");
        assert_eq!(
            RuntimeOp::resolve_member(&receiver, op.name(), |_| false),
            Some(op.unspecialized()),
            "`{receiver}.{}` resolves to {op}",
            op.name()
        );
        assert!(
            op.result_type(Some(&receiver), &sample_args(op)).is_some(),
            "{op} names a result type"
        );
        // With no receiver type, an element falls back to `Unknown`.
        let _ = op.result_type(None, &[]);
    }
}

#[test]
fn runtime_op_results_name_each_member_type() {
    let int64 = Type::named("int64");
    let float64 = Type::named("float64");
    let text = Type::named("str");
    let cases = [
        ("Duration", "to_ms", float64.clone()),
        ("Duration", "to_seconds", float64.clone()),
        ("float64", "sqrt", float64.clone()),
        ("int32", "to_float", float64.clone()),
        ("int32", "wrapping_add", Type::named("int32")),
        ("uint8", "saturating_shl", Type::named("uint8")),
        ("bool", "to_string", text.clone()),
        ("str", "clone", text.clone()),
        ("random.Rng", "next_int", int64.clone()),
        ("random.Rng", "next_float", float64.clone()),
        ("random.Rng", "shuffle", Type::Unit),
        ("fs.File", "read_all", result(text.clone(), "io.Error")),
        ("fs.File", "read_bytes", result(bytes(), "io.Error")),
        ("fs.File", "flush", result(Type::Unit, "io.Error")),
        ("fs.File", "close", Type::Unit),
        ("process.Completed", "stdout", text.clone()),
        (
            "net.UdpSocket",
            "recv",
            result(optional_type(bytes()), "io.Error"),
        ),
        (
            "net.TlsStream",
            "read_line",
            result(optional_type(text.clone()), "io.Error"),
        ),
        (
            "process.Child",
            "stdout",
            optional_type(Type::named("process.Pipe")),
        ),
        ("process.Child", "wait", Type::named("process.Wait")),
        (
            "process.Child",
            "wait_or_none",
            result(
                optional_type(Type::named("process.ExitStatus")),
                "process.Error",
            ),
        ),
        (
            "process.Child",
            "wait_ok",
            result(Type::named("process.ExitStatus"), "process.Error"),
        ),
        ("process.Pipe", "flush", result(Type::Unit, "process.Error")),
        (
            "process.Pipe",
            "read_all",
            result(text.clone(), "process.Error"),
        ),
        (
            "process.Pipe",
            "read_line",
            result(optional_type(text.clone()), "process.Error"),
        ),
        (
            "process.Pipe",
            "read_bytes",
            result(optional_type(bytes()), "process.Error"),
        ),
        ("process.Supervisor", "close", Type::Unit),
        (
            "process.Supervisor",
            "wait",
            Type::named("process.SupervisorWait"),
        ),
        (
            "process.Supervisor",
            "wait_or_none",
            result(
                optional_type(Type::named("process.SupervisorEvent")),
                "process.Error",
            ),
        ),
        ("process.Supervisor", "is_empty", Type::named("bool")),
    ];
    for (name, field, expected) in cases {
        let receiver = Type::named(name);
        let op = RuntimeOp::resolve_member(&receiver, field, |_| false)
            .unwrap_or_else(|| panic!("`{name}.{field}` is a runtime op"));
        assert_eq!(
            op.result_type(Some(&receiver), &[]),
            Some(expected),
            "{name}.{field}"
        );
    }
    let list = named("list", vec![int64]);
    assert_eq!(
        RuntimeOp::Value(ValueOp::Copy).result_type(Some(&list), &[]),
        Some(list)
    );
}

#[test]
fn class_methods_and_unnamed_receivers_shadow_runtime_ops() {
    let file = Type::named("fs.File");
    assert_eq!(
        RuntimeOp::resolve_member(&file, "read_all", |class| class == "fs.File"),
        None,
        "a class method shadows the built-in member"
    );
    assert_eq!(
        builtin_member_for_receiver(&Type::TypeParam("T".to_string()), "read_all", |_| {
            unreachable!("a type parameter names no class")
        }),
        None
    );
    assert_eq!(RuntimeOp::resolve_member(&file, "missing", |_| false), None);
    assert_eq!(
        RuntimeOp::resolve_member(&named("list", vec![]), "sort", |_| false),
        None,
        "`sort` lowers to a loop"
    );
    // A private collection member resolves only on the receiver types that
    // lower to it, under the same class precedence.
    let list = named("list", vec![Type::named("int64")]);
    assert_eq!(
        RuntimeOp::resolve_member(&list, "__index", |_| false),
        Some(RuntimeOp::List(ListOp::Index))
    );
    assert_eq!(RuntimeOp::resolve_member(&list, "__index", |_| true), None);
    assert_eq!(
        RuntimeOp::resolve_member(&list, "__span_fill", |_| false),
        Some(RuntimeOp::Span(SpanOp::Fill))
    );
    assert_eq!(
        RuntimeOp::resolve_member(&list, "__missing", |_| false),
        None
    );
    assert_eq!(
        RuntimeOp::resolve_member(&named("Array", vec![]), "__index_option", |_| false),
        None,
        "an `Array` has no iteration read"
    );
    assert_eq!(
        RuntimeOp::resolve_member(&named("set", vec![]), "__span_start", |_| false),
        None
    );
    assert_eq!(
        RuntimeOp::resolve_member(&Type::named("Range"), "__index", |_| false),
        None
    );
    assert_eq!(
        RuntimeOp::resolve_member(&Type::named("str"), "__slice", |_| false),
        Some(RuntimeOp::Str(StrOp::Slice))
    );
    assert_eq!(
        RuntimeOp::resolve_member(&named("dict", vec![]), "contains_key", |_| false),
        Some(RuntimeOp::Dict(DictOp::ContainsKey))
    );
    // A private queue receive resolves only on a `Queue`, under the same
    // class precedence.
    let queue = named("Queue", vec![Type::named("int64")]);
    assert_eq!(
        RuntimeOp::resolve_member(&queue, QueueOp::GET_IN_TASK_GROUP, |_| false),
        Some(RuntimeOp::Queue(QueueOp::GetInTaskGroup))
    );
    assert_eq!(
        RuntimeOp::resolve_member(&queue, QueueOp::GET_IN_TASK_GROUP, |_| true),
        None
    );
    assert_eq!(
        RuntimeOp::resolve_member(
            &Type::named("str"),
            QueueOp::GET_WITH_REGISTERED_PRODUCERS,
            |_| unreachable!("only a `Queue` receiver asks")
        ),
        None
    );
    assert_eq!(
        RuntimeOp::resolve_member(
            &Type::TypeParam("T".to_string()),
            QueueOp::GET_IN_TASK_GROUP,
            |_| unreachable!("a type parameter names no class")
        ),
        None
    );
}

#[test]
fn str_queue_and_task_ops_name_their_members_and_results() {
    let int64 = Type::named("int64");
    let text = Type::named("str");
    let queue = named("Queue", vec![int64.clone()]);
    let task = named("Task", vec![text.clone()]);
    let result = |op: RuntimeOp, receiver: &Type| op.result_type(Some(receiver), &[]);
    assert_eq!(
        result(RuntimeOp::Str(StrOp::Len), &text),
        Some(int64.clone())
    );
    assert_eq!(result(RuntimeOp::Str(StrOp::EndsWith), &text), some("bool"));
    assert_eq!(
        result(RuntimeOp::Str(StrOp::Split), &text),
        Some(named("list", vec![text.clone()]))
    );
    assert_eq!(
        result(RuntimeOp::Str(StrOp::StripSuffix), &text),
        Some(optional_type(text.clone()))
    );
    assert_eq!(result(RuntimeOp::Str(StrOp::Join), &text), some("str"));
    assert_eq!(
        result(RuntimeOp::Str(StrOp::SplitOnce), &text),
        Some(optional_type(Type::Tuple(vec![text.clone(), text.clone()])))
    );
    assert_eq!(result(RuntimeOp::Str(StrOp::Count), &text), some("int64"));
    let send = named(
        "Result",
        vec![Type::Unit, named("SendError", vec![int64.clone()])],
    );
    let cases = [
        (QueueOp::Put, Some(send.clone())),
        (QueueOp::PutTimeout, Some(send.clone())),
        (QueueOp::TryPut, Some(send)),
        (
            QueueOp::GetWithRegisteredProducers,
            Some(named("QueueReceive", vec![int64.clone()])),
        ),
        (
            QueueOp::PollTimeout,
            Some(named("Poll", vec![int64.clone()])),
        ),
        (QueueOp::GetOrTimeout, Some(int64.clone())),
        (QueueOp::Close, Some(Type::Unit)),
    ];
    for (op, expected) in cases {
        assert_eq!(result(RuntimeOp::Queue(op), &queue), expected, "{op:?}");
    }
    // A queue without a payload type names an unknown payload, and has no
    // `get_or` result.
    assert_eq!(
        result(RuntimeOp::Queue(QueueOp::Get), &Type::named("Queue")),
        Some(named("QueueReceive", vec![Type::named("Unknown")]))
    );
    assert_eq!(
        result(RuntimeOp::Queue(QueueOp::GetOr), &Type::named("Queue")),
        None
    );
    assert_eq!(
        result(RuntimeOp::Task(TaskOp::ResultTimeout), &task),
        Some(named("TaskResult", vec![text.clone()]))
    );
    assert_eq!(
        result(RuntimeOp::Task(TaskOp::Poll), &task),
        Some(named("Poll", vec![text.clone()]))
    );
    assert_eq!(
        result(RuntimeOp::Task(TaskOp::ResultOr), &task),
        Some(text.clone())
    );
    assert_eq!(
        RuntimeOp::Task(TaskOp::Result).result_type(None, &[]),
        Some(named("TaskResult", vec![Type::Unit]))
    );
    assert_eq!(
        result(
            RuntimeOp::TaskGroup(TaskGroupOp::Cancel),
            &Type::named("TaskGroup")
        ),
        Some(Type::Unit)
    );
}

#[test]
fn queue_and_task_timeout_forms_take_a_required_timeout() {
    let names = |op: RuntimeOp| {
        op.params()
            .into_iter()
            .map(|param| (param.name, param.required))
            .collect::<Vec<_>>()
    };
    let cases = [
        (RuntimeOp::Queue(QueueOp::Put), vec![("value", true)]),
        (
            RuntimeOp::Queue(QueueOp::PutTimeout),
            vec![("value", true), ("timeout", true)],
        ),
        (RuntimeOp::Queue(QueueOp::Get), vec![]),
        (
            RuntimeOp::Queue(QueueOp::GetTimeout),
            vec![("timeout", true)],
        ),
        (
            RuntimeOp::Queue(QueueOp::GetInTaskGroup),
            vec![("group", true)],
        ),
        (
            RuntimeOp::Queue(QueueOp::GetWithRegisteredProducers),
            vec![],
        ),
        (
            RuntimeOp::Task(TaskOp::ResultOrTimeout),
            vec![("default", true), ("timeout", true)],
        ),
        (RuntimeOp::Queue(QueueOp::Close), vec![]),
        (
            RuntimeOp::Str(StrOp::Replace),
            vec![("from", true), ("to", true)],
        ),
    ];
    for (op, expected) in cases {
        assert_eq!(names(op), expected, "{op}");
    }
    for op in RuntimeOp::all() {
        let base = op.unspecialized();
        assert_eq!(base.unspecialized(), base, "{op}");
        if op != base && !matches!(op, RuntimeOp::Print(_)) {
            // A collection op specializes by its receiver's element type
            // or a literal key, never by a timeout.
            if !matches!(
                op,
                RuntimeOp::List(_) | RuntimeOp::Dict(_) | RuntimeOp::Set(_)
            ) {
                assert_eq!(base.with_timeout(), op, "{op} is its base's timeout form");
            }
            assert_eq!(op.name(), base.name());
            assert_eq!(op.builtin_member(), base.builtin_member());
        }
    }
    assert_eq!(
        RuntimeOp::Task(TaskOp::Poll).with_timeout(),
        RuntimeOp::Task(TaskOp::PollTimeout)
    );
    assert_eq!(
        RuntimeOp::Queue(QueueOp::Close).with_timeout(),
        RuntimeOp::Queue(QueueOp::Close)
    );
    // A timeout form is its own timeout form.
    assert_eq!(
        RuntimeOp::Task(TaskOp::PollTimeout).with_timeout(),
        RuntimeOp::Task(TaskOp::PollTimeout)
    );
    assert_eq!(
        RuntimeOp::Queue(QueueOp::GetTimeout).with_timeout(),
        RuntimeOp::Queue(QueueOp::GetTimeout)
    );
    assert_eq!(
        RuntimeOp::Str(StrOp::Len).with_timeout(),
        RuntimeOp::Str(StrOp::Len)
    );
    // A private receive borrows its argument; `put` takes its value.
    assert_eq!(
        RuntimeOp::Queue(QueueOp::GetInTaskGroup).argument_passing(0),
        Some(crate::ast::ReceiverKind::Borrow)
    );
    assert_eq!(
        RuntimeOp::Queue(QueueOp::Put).argument_passing(0),
        Some(crate::ast::ReceiverKind::Value)
    );
}

#[test]
fn str_text_methods_pass_their_dispatcher_opcode() {
    for op in StrOp::ALL {
        let call = RuntimeOp::Str(*op).runtime_call();
        let opcode = op
            .builtin_member()
            .and_then(|member| crate::text_methods::text_method_opcode(member.name()));
        assert_eq!(op.is_text_method(), opcode.is_some(), "{op:?}");
        match opcode {
            Some(opcode) => {
                assert_eq!(call.function, RuntimeFn::StringTextMethod);
                assert_eq!(call.immediates, [opcode], "{op:?}");
            }
            None => assert!(call.immediates.is_empty(), "{op:?}"),
        }
    }
}

#[test]
fn runtime_ops_display_their_family() {
    let cases = [
        (
            RuntimeOp::Resource(ResourceOp::FileReadAll),
            "Resource.FileReadAll",
        ),
        (RuntimeOp::Rng(RngOp::Shuffle), "Rng.Shuffle"),
        (RuntimeOp::Scalar(ScalarOp::Sqrt), "Scalar.Sqrt"),
        (RuntimeOp::Value(ValueOp::Copy), "Value.Copy"),
        (RuntimeOp::Str(StrOp::TrimStart), "Str.TrimStart"),
        (
            RuntimeOp::Queue(QueueOp::GetOrTimeout),
            "Queue.GetOrTimeout",
        ),
        (RuntimeOp::Task(TaskOp::Poll), "Task.Poll"),
        (
            RuntimeOp::TaskGroup(TaskGroupOp::Cancel),
            "TaskGroup.Cancel",
        ),
        (
            RuntimeOp::List(ListOp::PushScalar(ScalarElement::Int64)),
            "List.PushScalar(Int64)",
        ),
        (RuntimeOp::Dict(DictOp::GetStr), "Dict.GetStr"),
        (RuntimeOp::Set(SetOp::ContainsInt64), "Set.ContainsInt64"),
        (RuntimeOp::Array(ArrayOp::Map), "Array.Map"),
        (RuntimeOp::Span(SpanOp::Extract), "Span.Extract"),
        (RuntimeOp::Function(FunctionOp::Range), "Function.Range"),
        (RuntimeOp::Print(PrintKind::Int), "Print.Int"),
        (
            RuntimeOp::Host(HostBuiltin::from_name("json::parse").unwrap()),
            "Host.json::parse",
        ),
    ];
    for (op, expected) in cases {
        assert_eq!(op.to_string(), expected);
    }
}

#[test]
fn collection_ops_specialize_by_element_type_and_literal_key() {
    let push = RuntimeOp::List(ListOp::Push);
    let index = RuntimeOp::List(ListOp::Index);
    for kind in ScalarElement::ALL {
        let element = scalar_element_type(*kind);
        assert_eq!(ScalarElement::of(&element), Some(*kind));
        let receiver = named("list", vec![element]);
        assert_eq!(
            specialized_member_op(push, &receiver, &[]),
            RuntimeOp::List(ListOp::PushScalar(*kind))
        );
        assert_eq!(
            specialized_member_op(index, &receiver, &[]),
            RuntimeOp::List(ListOp::IndexScalar(*kind))
        );
    }
    let text_list = named("list", vec![Type::named("str")]);
    assert_eq!(specialized_member_op(push, &text_list, &[]), push);
    assert_eq!(ScalarElement::of(&text_list), None);
    assert_eq!(ScalarElement::of(&Type::named("int32")), None);
    assert_eq!(ScalarElement::of(&Type::Unit), None);

    let contains = RuntimeOp::Set(SetOp::Contains);
    assert_eq!(
        specialized_member_op(contains, &named("set", vec![Type::named("int64")]), &[]),
        RuntimeOp::Set(SetOp::ContainsInt64)
    );
    assert_eq!(
        specialized_member_op(contains, &named("set", vec![Type::named("bool")]), &[]),
        contains
    );

    let get = RuntimeOp::Dict(DictOp::Get);
    let text_keys = named("dict", vec![Type::named("str"), Type::named("int64")]);
    let literal = [positional(Operand::String("key".to_string()))];
    let place = [positional(Operand::Place("key".to_string()))];
    assert_eq!(
        specialized_member_op(get, &text_keys, &literal),
        RuntimeOp::Dict(DictOp::GetStr)
    );
    assert_eq!(specialized_member_op(get, &text_keys, &place), get);
    let int_keys = named("dict", vec![Type::named("int64"), Type::named("int64")]);
    assert_eq!(specialized_member_op(get, &int_keys, &literal), get);
}

#[test]
fn collection_ops_pass_their_kind_and_operation_words() {
    use RuntimeFn as F;
    let cases: [(RuntimeOp, RuntimeFn, &[i64], usize); 14] = [
        (
            RuntimeOp::List(ListOp::PushScalar(ScalarElement::Float64)),
            F::VecPushScalar,
            &[1],
            0,
        ),
        (
            RuntimeOp::List(ListOp::IndexScalar(ScalarElement::Bool)),
            F::VecIndexScalar,
            &[2],
            0,
        ),
        (
            RuntimeOp::List(ListOp::Pop),
            F::CollectionOperation,
            &[0],
            1,
        ),
        (
            RuntimeOp::List(ListOp::Remove),
            F::CollectionOperation,
            &[1],
            1,
        ),
        (
            RuntimeOp::List(ListOp::IndexOf),
            F::CollectionOperation,
            &[2],
            1,
        ),
        (
            RuntimeOp::List(ListOp::Count),
            F::CollectionOperation,
            &[3],
            1,
        ),
        (
            RuntimeOp::Dict(DictOp::Reserve),
            F::CollectionOperation,
            &[4],
            1,
        ),
        (
            RuntimeOp::Set(SetOp::Discard),
            F::CollectionOperation,
            &[6],
            1,
        ),
        (
            RuntimeOp::Set(SetOp::Clear),
            F::CollectionOperation,
            &[7],
            2,
        ),
        (RuntimeOp::Dict(DictOp::GetStr), F::MapGetStr, &[], 1),
        (RuntimeOp::Array(ArrayOp::Mean), F::ArrayReduce, &[3], 0),
        (
            RuntimeOp::Array(ArrayOp::SaturatingMul),
            F::ArrayBinary,
            &[0, 2, 2],
            0,
        ),
        (RuntimeOp::Span(SpanOp::Fill), F::VecSpan, &[9], 6),
        (RuntimeOp::Span(SpanOp::Start), F::VecSpan, &[0], 1),
    ];
    for (op, function, immediates, layout_words) in cases {
        let call = op.runtime_call();
        assert_eq!(call.function, function, "{op}");
        assert_eq!(call.immediates, immediates, "{op}");
        assert_eq!(call.layout_words, layout_words, "{op}");
    }
    for op in SpanOp::ALL {
        let mutates = matches!(op, SpanOp::Extract | SpanOp::Restore | SpanOp::Fill);
        assert_eq!(op.mutates_receiver(), mutates, "{op:?}");
    }
}

fn some(name: &str) -> Option<Type> {
    Some(Type::named(name))
}

#[test]
fn named_builtins_resolve_by_precedence_and_yield_to_user_functions() {
    let none_declared = |_: &str| false;
    let cases = [
        ("print", RuntimeOp::Print(PrintKind::Value)),
        ("range", RuntimeOp::Function(FunctionOp::Range)),
        ("wait_any", RuntimeOp::Function(FunctionOp::WaitAny)),
        (
            "Duration.ms",
            RuntimeOp::Function(FunctionOp::DurationMilliseconds),
        ),
        (
            "Array.from_list",
            RuntimeOp::Function(FunctionOp::ArrayFromList),
        ),
        ("set", RuntimeOp::Function(FunctionOp::SetEmpty)),
        ("fs::exists", RuntimeOp::Function(FunctionOp::FsExists)),
        // A dedicated helper wins over the host table entry of one name.
        (
            "sys::monotonic_time_ms",
            RuntimeOp::Function(FunctionOp::MonotonicTimeMs),
        ),
    ];
    for (name, op) in cases {
        assert_eq!(
            builtin_function_for_name(name, none_declared),
            Some(op),
            "{name}"
        );
        assert_eq!(op.name(), name);
    }
    let parse = builtin_function_for_name("json::parse", none_declared)
        .expect("a host builtin resolves by its qualified name");
    assert!(matches!(parse, RuntimeOp::Host(builtin) if builtin.name() == "json::parse"));
    assert_eq!(builtin_function_for_name("missing", none_declared), None);
    // A user function of the same name shadows the builtin.
    assert_eq!(builtin_function_for_name("set", |name| name == "set"), None);
    assert_eq!(builtin_function_for_name("print", |_| true), None);
    assert_eq!(
        builtin_function_for_name("list", |name| name == "set"),
        Some(RuntimeOp::Function(FunctionOp::ListEmpty))
    );
}

#[test]
fn print_kinds_follow_the_argument_layout() {
    let cases = [
        (some("int32"), PrintKind::Int),
        (some("int64"), PrintKind::Int),
        (some("uint64"), PrintKind::Uint64),
        (some("float32"), PrintKind::Float32),
        (some("float64"), PrintKind::Float64),
        (some("bool"), PrintKind::Bool),
        (some("str"), PrintKind::Value),
        (
            Some(named("list", vec![Type::named("int64")])),
            PrintKind::Value,
        ),
        (Some(Type::Unit), PrintKind::Value),
        (None, PrintKind::Value),
    ];
    for (ty, kind) in cases {
        assert_eq!(PrintKind::for_type(ty.as_ref()), kind, "{ty:?}");
    }
    let helpers = PrintKind::ALL
        .iter()
        .map(|kind| kind.runtime_fn())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(helpers.len(), PrintKind::ALL.len());
    assert_eq!(PrintKind::Value.runtime_fn(), RuntimeFn::PrintValue);
    for kind in PrintKind::ALL {
        let op = RuntimeOp::Print(*kind);
        assert_eq!(op.name(), "print");
        assert_eq!(op.unspecialized(), RuntimeOp::Print(PrintKind::Value));
        assert_eq!(op.result_type(None, &[]), Some(Type::Unit));
    }
}

#[test]
fn function_op_result_types_follow_their_arguments() {
    let result =
        |op: FunctionOp, args: &[Option<Type>]| RuntimeOp::Function(op).result_type(None, args);
    let int64 = Type::named("int64");
    let tasks = |payload: Type| Some(named("list", vec![named("Task", vec![payload])]));
    assert_eq!(result(FunctionOp::RngNew, &[]), some("random.Rng"));
    assert_eq!(result(FunctionOp::DurationSeconds, &[]), some("Duration"));
    assert_eq!(result(FunctionOp::Range, &[]), some("Range"));
    assert_eq!(result(FunctionOp::TaskGroupNew, &[]), some("TaskGroup"));
    assert_eq!(result(FunctionOp::Cancelled, &[]), some("bool"));
    assert_eq!(result(FunctionOp::YieldNow, &[]), Some(Type::Unit));
    assert_eq!(result(FunctionOp::Sleep, &[]), Some(Type::Unit));
    assert_eq!(
        result(
            FunctionOp::Select,
            &[
                Some(named("Queue", vec![Type::named("str")])),
                Some(named("Task", vec![int64.clone()])),
                some("Duration"),
                None,
            ]
        ),
        Some(named(
            "SelectOutcome",
            vec![Type::named("str"), int64.clone()]
        ))
    );
    assert_eq!(
        result(FunctionOp::Select, &[some("Duration")]),
        Some(named("SelectOutcome", vec![Type::Unit, Type::Unit]))
    );
    assert_eq!(
        result(FunctionOp::WaitAny, &[tasks(int64.clone())]),
        Some(named("WaitAny", vec![int64.clone()]))
    );
    assert_eq!(
        result(
            FunctionOp::WaitAllTimeout,
            &[tasks(int64.clone()), some("Duration")]
        ),
        Some(named("WaitAll", vec![int64.clone()]))
    );
    for args in [
        vec![],
        vec![some("int64")],
        vec![Some(named("list", vec![int64.clone()]))],
        vec![Some(named("set", vec![named("Task", vec![int64.clone()])]))],
        vec![Some(named("list", vec![named("Task", vec![])]))],
    ] {
        assert_eq!(result(FunctionOp::WaitAny, &args), None, "{args:?}");
    }
    for op in [
        FunctionOp::Abs,
        FunctionOp::Min,
        FunctionOp::Max,
        FunctionOp::Sqrt,
    ] {
        assert_eq!(result(op, &[some("int8"), some("int8")]), some("int8"));
        assert_eq!(result(op, &[]), None);
    }
    assert_eq!(result(FunctionOp::Round, &[some("float32")]), some("int64"));
    assert_eq!(result(FunctionOp::Round, &[some("int16")]), some("int16"));
    assert_eq!(result(FunctionOp::Round, &[None]), None);
    assert_eq!(
        result(FunctionOp::Divmod, &[some("int8"), some("int8")]),
        Some(Type::Tuple(vec![Type::named("int8"), Type::named("int8")]))
    );
    assert_eq!(result(FunctionOp::Divmod, &[]), None);
    assert_eq!(
        result(FunctionOp::ParseInt32, &[]),
        Some(result_of("int32"))
    );
    assert_eq!(
        result(FunctionOp::ParseInt64, &[]),
        Some(result_of("int64"))
    );
    assert_eq!(
        result(FunctionOp::ParseFloat64, &[]),
        Some(result_of("float64"))
    );
    for op in [
        FunctionOp::ArrayZeros,
        FunctionOp::ArrayFull,
        FunctionOp::ArrayFromList,
        FunctionOp::ListWithCapacity,
        FunctionOp::DictWithCapacity,
        FunctionOp::SetWithCapacity,
        FunctionOp::ListEmpty,
        FunctionOp::DictEmpty,
        FunctionOp::SetEmpty,
        FunctionOp::QueueNew,
    ] {
        assert_eq!(result(op, &[]), None, "{op:?} takes its type from the call");
    }
    // A receiver op asked for its result without a receiver has none.
    assert_eq!(
        RuntimeOp::Scalar(ScalarOp::WrappingAdd).result_type(None, &[]),
        None
    );
    // Module functions read their declared signature.
    assert_eq!(result(FunctionOp::FsExists, &[]), some("bool"));
    assert_eq!(result(FunctionOp::MonotonicTimeMs, &[]), some("int64"));
}

fn result_of(ok: &str) -> Type {
    result(Type::named(ok), "str")
}

#[test]
fn function_op_params_by_category() {
    let names = |op: FunctionOp| {
        RuntimeOp::Function(op)
            .params()
            .into_iter()
            .map(|param| (param.name, param.required))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(FunctionOp::QueueNew), [("capacity", false)]);
    assert_eq!(names(FunctionOp::WaitAny), [("tasks", true)]);
    assert_eq!(
        names(FunctionOp::WaitAllTimeout),
        [("tasks", true), ("timeout", true)]
    );
    for op in [
        FunctionOp::TaskGroupNew,
        FunctionOp::ListEmpty,
        FunctionOp::DictEmpty,
        FunctionOp::SetEmpty,
        FunctionOp::Cancelled,
    ] {
        assert_eq!(names(op), [], "{op:?}");
    }
    assert_eq!(names(FunctionOp::RngNew), [("seed", true)]);
    assert_eq!(names(FunctionOp::Min), [("left", true), ("right", true)]);
    assert_eq!(
        names(FunctionOp::ArrayFromList),
        [("values", true), ("shape", true)]
    );
    assert_eq!(names(FunctionOp::FsExists), [("path", true)]);
    assert_eq!(
        RuntimeOp::Print(PrintKind::Int)
            .params()
            .into_iter()
            .map(|param| param.name)
            .collect::<Vec<_>>(),
        ["value"]
    );
}

#[test]
fn function_op_specializations_and_collections() {
    for op in FunctionOp::ALL {
        assert_eq!(op.unspecialized().unspecialized(), op.unspecialized());
        let runtime_op = RuntimeOp::Function(*op);
        assert!(!runtime_op.has_receiver());
        assert!(!runtime_op.applies_to("list"));
        assert_eq!(runtime_op.builtin_member(), None);
    }
    assert_eq!(
        FunctionOp::WaitAny.with_timeout(),
        FunctionOp::WaitAnyTimeout
    );
    assert_eq!(
        FunctionOp::WaitAll.with_timeout(),
        FunctionOp::WaitAllTimeout
    );
    assert_eq!(FunctionOp::Sleep.with_timeout(), FunctionOp::Sleep);
    assert_eq!(
        FunctionOp::WaitAnyTimeout.unspecialized(),
        FunctionOp::WaitAny
    );
    assert_eq!(FunctionOp::WaitAnyTimeout.name(), "wait_any");
    assert_eq!(
        FunctionOp::WaitAllTimeout.runtime_fn(),
        RuntimeFn::WaitAllTimeoutValue
    );
    let cases = [
        (FunctionOp::ListEmpty, Some(RuntimeFn::VecEmpty)),
        (FunctionOp::ListWithCapacity, Some(RuntimeFn::VecEmpty)),
        (FunctionOp::DictEmpty, Some(RuntimeFn::MapEmpty)),
        (FunctionOp::DictWithCapacity, Some(RuntimeFn::MapEmpty)),
        (FunctionOp::SetEmpty, Some(RuntimeFn::SetEmpty)),
        (FunctionOp::SetWithCapacity, Some(RuntimeFn::SetEmpty)),
        (FunctionOp::Range, None),
    ];
    for (op, empty) in cases {
        assert_eq!(op.empty_collection_fn(), empty, "{op:?}");
    }
}

#[test]
fn host_builtins_round_trip_through_their_names() {
    let mut count = 0;
    for builtin in HostBuiltin::all() {
        count += 1;
        assert_eq!(HostBuiltin::from_name(builtin.name()), Some(builtin));
        assert!(
            builtin.symbol().starts_with("aura_host_"),
            "{}",
            builtin.symbol()
        );
        let op = RuntimeOp::Host(builtin);
        assert_eq!(op.name(), builtin.name());
        assert!(!op.has_receiver());
        assert_eq!(op.unspecialized(), op);
        assert_eq!(op.builtin_member(), None);
        assert_eq!(op.runtime_target(), RuntimeTarget::Host(builtin.symbol()));
        assert_eq!(op.params().len(), builtin.metadata().params.len());
        assert_eq!(
            op.result_type(None, &[]).as_ref(),
            Some(&builtin.metadata().return_type)
        );
        let json = serde_json::to_string(&builtin).unwrap();
        assert_eq!(json, format!("\"{}\"", builtin.name()));
        assert_eq!(serde_json::from_str::<HostBuiltin>(&json).unwrap(), builtin);
    }
    assert_eq!(count + 1, HOST_BUILTIN_SYMBOLS.len());
    assert_eq!(HostBuiltin::from_name("sys::monotonic_time_ms"), None);
    assert_eq!(HostBuiltin::from_name("json::missing"), None);
    let error = serde_json::from_str::<HostBuiltin>("\"json::missing\"").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unknown host builtin `json::missing`"),
        "{error}"
    );
    assert!(serde_json::from_str::<HostBuiltin>("7").is_err());
}

#[test]
#[should_panic(expected = "calls its own entry")]
fn a_host_builtin_has_no_runtime_helper() {
    let builtin = HostBuiltin::from_name("json::parse").unwrap();
    let _ = RuntimeOp::Host(builtin).runtime_call();
}

#[test]
fn only_fixed_width_integer_ops_name_integer_width_words() {
    for &op in ScalarOp::ALL {
        let words = op.integer_width_words();
        match op {
            ScalarOp::ToFloat
            | ScalarOp::Sqrt
            | ScalarOp::DurationToMilliseconds
            | ScalarOp::DurationToSeconds => {
                assert_eq!(words, None, "{op:?} is not a fixed-width integer op");
            }
            _ => {
                let words = words.unwrap_or_else(|| panic!("{op:?} names its words"));
                assert_eq!(words.len(), 2, "{op:?} names an operation and a mode");
                assert_eq!(
                    RuntimeOp::Scalar(op).runtime_call().immediates,
                    words,
                    "{op:?} passes its words to the helper"
                );
            }
        }
    }
    assert_eq!(
        ScalarOp::SaturatingMul.integer_width_words(),
        Some(&[2, 2][..])
    );
    assert_eq!(
        ScalarOp::WrappingShr.integer_width_words(),
        Some(&[4, 1][..])
    );
}

#[test]
fn every_op_states_the_ownership_of_each_parameter_and_its_receiver() {
    use crate::mir::ownership::OperandRole;
    for op in RuntimeOp::all() {
        let params = op.params();
        for index in 0..params.len() {
            assert!(
                op.arg_ownership(index).is_some(),
                "`{op}` states no ownership for parameter {index}"
            );
        }
        if op != RuntimeOp::Function(FunctionOp::Select) {
            assert_eq!(
                op.arg_ownership(params.len()),
                None,
                "`{op}` has no parameter {}",
                params.len()
            );
        }
        assert_eq!(
            op.receiver_ownership(),
            op.has_receiver().then_some(OperandRole::Borrow),
            "`{op}` borrows its receiver"
        );
        // A member op consumes exactly its `own` parameters.
        if let Some(member) = op.builtin_member() {
            for index in 0..params.len() {
                assert_eq!(
                    op.arg_ownership(index),
                    member.argument_passing(index).map(OperandRole::of_passing),
                    "`{op}` parameter {index}"
                );
            }
        }
    }
}

#[test]
fn ops_consume_the_values_they_store() {
    use crate::mir::ownership::OperandRole::{Borrow, Consume};
    let roles = |op: RuntimeOp| {
        (0..op.params().len())
            .map(|index| op.arg_ownership(index).expect("a declared parameter"))
            .collect::<Vec<_>>()
    };
    assert_eq!(roles(RuntimeOp::List(ListOp::Push)), [Consume]);
    assert_eq!(
        roles(RuntimeOp::List(ListOp::PushScalar(ScalarElement::Int64))),
        [Consume]
    );
    assert_eq!(
        roles(RuntimeOp::List(ListOp::SetIndex)),
        [Borrow, Consume, Borrow, Borrow]
    );
    assert_eq!(
        roles(RuntimeOp::Dict(DictOp::SetIndex)),
        [Consume, Consume, Borrow, Borrow]
    );
    assert_eq!(
        roles(RuntimeOp::Array(ArrayOp::SetIndex)),
        [Borrow, Consume, Borrow, Borrow]
    );
    assert_eq!(roles(RuntimeOp::Span(SpanOp::Restore)), [Borrow, Consume]);
    assert_eq!(roles(RuntimeOp::Span(SpanOp::Fill)), [Borrow; 3]);
    assert_eq!(roles(RuntimeOp::List(ListOp::Index)), [Borrow; 3]);
    assert_eq!(roles(RuntimeOp::Dict(DictOp::Index)), [Borrow; 3]);
    assert_eq!(roles(RuntimeOp::Set(SetOp::TakeIndexOption)), [Borrow]);
    assert_eq!(roles(RuntimeOp::Array(ArrayOp::Slice)), [Borrow; 6]);
    assert_eq!(roles(RuntimeOp::Str(StrOp::Slice)), [Borrow; 6]);
    assert_eq!(roles(RuntimeOp::Queue(QueueOp::GetInTaskGroup)), [Borrow]);
    assert_eq!(roles(RuntimeOp::Print(PrintKind::Value)), [Borrow]);
    assert_eq!(
        RuntimeOp::Function(FunctionOp::Select).arg_ownership(5),
        Some(Borrow),
        "`select` borrows every source"
    );
    assert_eq!(roles(RuntimeOp::Function(FunctionOp::RngNew)), [Borrow]);
    assert_eq!(roles(RuntimeOp::Function(FunctionOp::QueueNew)), [Borrow]);
    assert_eq!(
        roles(RuntimeOp::Function(FunctionOp::ArrayFull)),
        [Borrow; 2]
    );
    assert_eq!(roles(RuntimeOp::Function(FunctionOp::FsExists)), [Borrow]);
    assert_eq!(
        RuntimeOp::Function(FunctionOp::FsExists).arg_ownership(1),
        None
    );
    let host = HostBuiltin::all()
        .next()
        .expect("the runtime has host builtins");
    assert_eq!(
        RuntimeOp::Host(host).arg_ownership(0),
        host.metadata()
            .params
            .first()
            .map(|param| crate::mir::ownership::OperandRole::of_passing(param.passing))
    );
}
