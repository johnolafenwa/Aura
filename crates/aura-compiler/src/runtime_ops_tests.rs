use super::{builtin_member_for_receiver, ResourceOp, RngOp, RuntimeOp, ScalarOp, ValueOp};
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
    }
}

#[test]
fn every_runtime_op_resolves_from_its_receiver_and_has_a_result_type() {
    for op in RuntimeOp::all() {
        let receiver = sample_receiver(op);
        let Type::Named(name, _) = &receiver else {
            unreachable!("sample receivers are named types")
        };
        assert!(op.applies_to(name), "{op} applies to `{receiver}`");
        assert!(!op.applies_to("Queue"), "{op} does not apply to `Queue`");
        assert_eq!(
            RuntimeOp::resolve_member(&receiver, op.member_name(), |_| false),
            Some(op),
            "`{receiver}.{}` resolves to {op}",
            op.member_name()
        );
        assert!(
            op.result_type(&receiver).is_some(),
            "{op} names a result type"
        );
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
        assert_eq!(op.result_type(&receiver), Some(expected), "{name}.{field}");
    }
    let list = named("list", vec![int64]);
    assert_eq!(
        RuntimeOp::Value(ValueOp::Copy).result_type(&list),
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
        RuntimeOp::resolve_member(&Type::named("Queue"), "get", |_| false),
        None,
        "Queue ops are not resolved yet"
    );
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
    ];
    for (op, expected) in cases {
        assert_eq!(op.to_string(), expected);
    }
}
