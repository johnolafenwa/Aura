use super::{builtin_member_argument_type, runtime_member_result_type};
use crate::call::BuiltinMember;
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

#[test]
fn runtime_member_results_name_each_member_type() {
    let int64 = Type::named("int64");
    let cases = [
        ("Duration", vec![], "to_ms", Some(Type::named("float64"))),
        (
            "Duration",
            vec![],
            "to_seconds",
            Some(Type::named("float64")),
        ),
        ("random.Rng", vec![], "next_int", Some(int64.clone())),
        (
            "random.Rng",
            vec![],
            "next_float",
            Some(Type::named("float64")),
        ),
        ("random.Rng", vec![], "shuffle", Some(Type::Unit)),
        (
            "list",
            vec![int64.clone()],
            "__slice",
            Some(named("list", vec![int64.clone()])),
        ),
        ("Queue", vec![int64.clone()], "get_or", Some(int64.clone())),
        ("Queue", vec![], "get_or", None),
        (
            "process.Child",
            vec![],
            "stdout",
            Some(optional_type(Type::named("process.Pipe"))),
        ),
        (
            "process.Child",
            vec![],
            "wait",
            Some(Type::named("process.Wait")),
        ),
        (
            "process.Child",
            vec![],
            "wait_or_none",
            Some(result(
                optional_type(Type::named("process.ExitStatus")),
                "process.Error",
            )),
        ),
        (
            "process.Child",
            vec![],
            "wait_ok",
            Some(result(Type::named("process.ExitStatus"), "process.Error")),
        ),
        (
            "process.Pipe",
            vec![],
            "flush",
            Some(result(Type::Unit, "process.Error")),
        ),
        ("process.Supervisor", vec![], "close", Some(Type::Unit)),
        (
            "process.Pipe",
            vec![],
            "read_all",
            Some(result(Type::named("str"), "process.Error")),
        ),
        (
            "process.Pipe",
            vec![],
            "read_line",
            Some(result(optional_type(Type::named("str")), "process.Error")),
        ),
        (
            "process.Pipe",
            vec![],
            "read_bytes",
            Some(result(optional_type(bytes()), "process.Error")),
        ),
        (
            "process.Supervisor",
            vec![],
            "wait",
            Some(Type::named("process.SupervisorWait")),
        ),
        (
            "process.Supervisor",
            vec![],
            "wait_or_none",
            Some(result(
                optional_type(Type::named("process.SupervisorEvent")),
                "process.Error",
            )),
        ),
        (
            "process.Supervisor",
            vec![],
            "is_empty",
            Some(Type::named("bool")),
        ),
        ("process.Pipe", vec![], "missing", None),
    ];
    for (name, args, field, expected) in cases {
        assert_eq!(
            runtime_member_result_type(name, &args, field),
            expected,
            "{name}.{field}"
        );
    }
}

#[test]
fn builtin_member_arguments_name_open_collection_types() {
    let int64 = Type::named("int64");
    let text = Type::named("str");
    let headers = named("dict", vec![text.clone(), text.clone()]);
    let cases = [
        (
            BuiltinMember::StringJoin,
            0,
            vec![],
            Some(named("list", vec![text.clone()])),
        ),
        (
            BuiltinMember::VecExtend,
            0,
            vec![int64.clone()],
            Some(named("list", vec![int64.clone()])),
        ),
        (BuiltinMember::VecExtend, 0, vec![], None),
        (
            BuiltinMember::MapExtend,
            0,
            vec![text.clone(), int64.clone()],
            Some(named("dict", vec![text.clone(), int64.clone()])),
        ),
        (BuiltinMember::FileWriteBytes, 0, vec![], Some(bytes())),
        (BuiltinMember::UdpSocketSendBytes, 1, vec![], Some(bytes())),
        (
            BuiltinMember::HttpExchangeRespondText,
            2,
            vec![],
            Some(headers.clone()),
        ),
        (
            BuiltinMember::ProcessSupervisorStart,
            3,
            vec![],
            Some(headers),
        ),
        (
            BuiltinMember::ProcessSupervisorStart,
            1,
            vec![],
            Some(named("list", vec![text])),
        ),
        (BuiltinMember::StringJoin, 1, vec![], None),
    ];
    for (member, index, class_args, expected) in cases {
        assert_eq!(
            builtin_member_argument_type(member, index, &class_args),
            expected,
            "{member:?} argument {index}"
        );
    }
}
