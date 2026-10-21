use super::{builtin_member_argument_type, runtime_member_result_type};
use crate::call::BuiltinMember;
use crate::sema::Type;

fn named(name: &str, args: Vec<Type>) -> Type {
    Type::Named(name.to_string(), args)
}

fn bytes() -> Type {
    named("list", vec![Type::named("uint8")])
}

#[test]
fn runtime_member_results_name_each_member_type() {
    let int64 = Type::named("int64");
    // Duration, `random.Rng`, and process members are resolved runtime
    // operations; `runtime_ops_tests.rs` checks their result types.
    let cases = [
        (
            "list",
            vec![int64.clone()],
            "__slice",
            Some(named("list", vec![int64.clone()])),
        ),
        ("Queue", vec![int64.clone()], "get_or", Some(int64.clone())),
        ("Queue", vec![], "get_or", None),
        ("process.Pipe", vec![], "read_all", None),
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
