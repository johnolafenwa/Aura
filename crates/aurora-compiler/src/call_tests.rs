use crate::ast::{Argument, Expr, ExprKind, Param, ParamMode, ReceiverKind, TypeRef};
use crate::diag::Span;

use super::{
    bind_call_arguments, callable_params_from_decl, format_argument_count, BuiltinFunction,
    BuiltinMember, CallConvention, CallableParam, ALL_BUILTIN_FUNCTIONS,
};

fn dummy_arg(name: Option<&str>) -> Argument {
    Argument {
        name: name.map(|value| value.to_string()),
        value: Expr {
            kind: ExprKind::Int(1),
            span: Span::new(1, 1),
        },
        span: Span::new(1, 1),
    }
}

fn dummy_type(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args: vec![],
        indirect: false,
        span: Span::new(1, 1),
    }
}

#[test]
fn positional_or_named_binding_orders_arguments() {
    let params = [
        CallableParam::required("left"),
        CallableParam::required("right"),
    ];
    let args = [dummy_arg(None), dummy_arg(Some("right"))];
    let ordered = bind_call_arguments(
        "function `add`",
        &params,
        &args,
        Span::new(1, 1),
        CallConvention::PositionalOrNamed,
    )
    .expect("binding should succeed");

    assert!(ordered[0].is_some());
    assert!(ordered[1].is_some());
    assert_eq!(ordered[1].unwrap().name.as_deref(), Some("right"));
}

#[test]
fn keyword_only_binding_rejects_positional_arguments() {
    let params = [CallableParam::required("value")];
    let error = bind_call_arguments(
        "class constructor `Box`",
        &params,
        &[dummy_arg(None)],
        Span::new(1, 1),
        CallConvention::KeywordOnly,
    )
    .expect_err("binding should fail");

    assert!(error.message.contains("must be named"));
}

#[test]
fn callable_params_follow_default_presence() {
    let params = vec![
        Param {
            name: "required".to_string(),
            mode: ParamMode::Default,
            borrow_label: None,
            ty: dummy_type("int32"),
            default: None,
            span: Span::new(1, 1),
        },
        Param {
            name: "optional".to_string(),
            mode: ParamMode::Default,
            borrow_label: None,
            ty: dummy_type("int32"),
            default: Some(Expr {
                kind: ExprKind::Int(1),
                span: Span::new(1, 1),
            }),
            span: Span::new(1, 1),
        },
    ];

    let callable = callable_params_from_decl(&params);
    assert_eq!(callable[0], CallableParam::required("required"));
    assert_eq!(callable[1], CallableParam::optional("optional"));
}

#[test]
fn bind_call_arguments_reports_named_binding_errors() {
    let params = [
        CallableParam::required("left"),
        CallableParam::optional("right"),
    ];

    let duplicate = bind_call_arguments(
        "function `pair`",
        &params,
        &[dummy_arg(Some("left")), dummy_arg(Some("left"))],
        Span::new(1, 1),
        CallConvention::PositionalOrNamed,
    )
    .unwrap_err();
    assert!(duplicate.message.contains("provided more than once"));

    let unknown = bind_call_arguments(
        "function `pair`",
        &params,
        &[dummy_arg(Some("missing"))],
        Span::new(1, 1),
        CallConvention::PositionalOrNamed,
    )
    .unwrap_err();
    assert!(unknown.message.contains("has no parameter named"));

    let missing = bind_call_arguments(
        "function `pair`",
        &params,
        &[],
        Span::new(1, 1),
        CallConvention::PositionalOrNamed,
    )
    .unwrap_err();
    assert!(missing.message.contains("missing required argument"));

    let positional_after_named = bind_call_arguments(
        "function `pair`",
        &params,
        &[dummy_arg(Some("right")), dummy_arg(None)],
        Span::new(1, 1),
        CallConvention::PositionalOrNamed,
    )
    .unwrap_err();
    assert!(positional_after_named
        .message
        .contains("positional arguments must come before named arguments"));

    let positional_only_named = bind_call_arguments(
        "builtin `channel`",
        &[CallableParam::required("value")],
        &[dummy_arg(Some("value"))],
        Span::new(1, 1),
        CallConvention::PositionalOnly,
    )
    .unwrap_err();
    assert!(positional_only_named
        .message
        .contains("does not take keyword arguments"));

    let too_many = bind_call_arguments(
        "function `pair`",
        &params,
        &[dummy_arg(None), dummy_arg(None), dummy_arg(None)],
        Span::new(1, 1),
        CallConvention::PositionalOrNamed,
    )
    .unwrap_err();
    assert!(too_many.message.contains("expects 2 arguments, found 3"));

    let overlap = bind_call_arguments(
        "function `pair`",
        &params,
        &[dummy_arg(None), dummy_arg(Some("left"))],
        Span::new(1, 1),
        CallConvention::PositionalOrNamed,
    )
    .unwrap_err();
    assert!(overlap.message.contains("provided more than once"));
}

#[test]
fn builtin_function_metadata_and_binding_surface_are_stable() {
    for builtin in ALL_BUILTIN_FUNCTIONS {
        assert_eq!(BuiltinFunction::from_name(builtin.name()), Some(*builtin));
        assert!(!builtin.detail().is_empty());
        assert!(!builtin.docs().is_empty());
    }
    assert_eq!(BuiltinFunction::from_name("missing_builtin"), None);

    let range_two_args_input = [dummy_arg(Some("start")), dummy_arg(Some("stop"))];
    let range_two_args = BuiltinFunction::Range
        .bind_args(&range_two_args_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(range_two_args.len(), 2);

    let range_one_arg_input = [dummy_arg(None)];
    let range_one_arg = BuiltinFunction::Range
        .bind_args(&range_one_arg_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(range_one_arg.len(), 1);

    let range_error = BuiltinFunction::Range
        .bind_args(
            &[dummy_arg(None), dummy_arg(None), dummy_arg(None)],
            Span::new(1, 1),
        )
        .unwrap_err();
    assert!(range_error.message.contains("expects 1 or 2 arguments"));

    let wait_one_arg_input = [dummy_arg(None)];
    let wait_any_one_arg = BuiltinFunction::WaitAny
        .bind_args(&wait_one_arg_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(wait_any_one_arg.len(), 2);

    let wait_two_arg_input = [dummy_arg(None), dummy_arg(Some("timeout"))];
    let wait_all_two_arg = BuiltinFunction::WaitAll
        .bind_args(&wait_two_arg_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(wait_all_two_arg.len(), 2);

    let wait_error = BuiltinFunction::WaitAny
        .bind_args(
            &[dummy_arg(None), dummy_arg(None), dummy_arg(None)],
            Span::new(1, 1),
        )
        .unwrap_err();
    assert!(wait_error
        .message
        .contains("`wait_any` expects 2 arguments"));
}

#[test]
fn builtin_function_bind_args_cover_remaining_variants() {
    for builtin in [
        BuiltinFunction::Print,
        BuiltinFunction::Sleep,
        BuiltinFunction::Abs,
        BuiltinFunction::Sqrt,
        BuiltinFunction::ParseInt32,
        BuiltinFunction::ParseInt64,
        BuiltinFunction::ParseFloat64,
    ] {
        let args = [dummy_arg(None)];
        let bound = builtin
            .bind_args(&args, Span::new(1, 1))
            .expect("builtin should bind");
        assert_eq!(bound.len(), 1);
    }

    for builtin in [BuiltinFunction::Min, BuiltinFunction::Max] {
        let args = [dummy_arg(None), dummy_arg(None)];
        let bound = builtin
            .bind_args(&args, Span::new(1, 1))
            .expect("builtin should bind");
        assert_eq!(bound.len(), 2);
    }

    for builtin in [BuiltinFunction::Cancelled] {
        let bound = builtin
            .bind_args(&[], Span::new(1, 1))
            .expect("builtin should bind");
        assert!(bound.is_empty());
    }

    for builtin in [BuiltinFunction::WaitAny, BuiltinFunction::WaitAll] {
        let args = [dummy_arg(None), dummy_arg(Some("timeout"))];
        let bound = builtin
            .bind_args(&args, Span::new(1, 1))
            .expect("builtin should bind");
        assert_eq!(bound.len(), 2);
    }
}

#[test]
fn call_binding_helpers_cover_argument_count_and_decl_metadata_paths() {
    let params = vec![Param {
        name: "value".to_string(),
        mode: ParamMode::Borrow,
        borrow_label: None,
        ty: dummy_type("String"),
        default: None,
        span: Span::new(2, 4),
    }];
    let callable = callable_params_from_decl(&params);
    assert_eq!(callable, vec![CallableParam::required("value")]);

    let single_error = bind_call_arguments(
        "builtin `print`",
        &[CallableParam::required("value")],
        &[dummy_arg(None), dummy_arg(None)],
        Span::new(1, 1),
        CallConvention::PositionalOnly,
    )
    .expect_err("too many arguments should fail");
    assert!(single_error.message.contains("expects 1 argument, found 2"));

    let zero_error = BuiltinFunction::Cancelled
        .bind_args(&[dummy_arg(None)], Span::new(1, 1))
        .expect_err("zero-arg builtin should reject positional args");
    assert!(zero_error.message.contains("expects 0 arguments, found 1"));
}

#[test]
fn call_metadata_helpers_cover_argument_count_and_doc_surface() {
    assert_eq!(format_argument_count(1), "1 argument");
    assert_eq!(format_argument_count(2), "2 arguments");

    assert_eq!(
        BuiltinFunction::ParseFloat64.detail(),
        "parse_float64(text: String) -> Result[float64, String]"
    );
    assert!(BuiltinFunction::WaitAny
        .docs()
        .contains("first task to complete"));

    assert_eq!(
        BuiltinMember::StringContains.detail(),
        "contains(text: String) -> bool"
    );
    assert!(BuiltinMember::MapEntries.docs().contains("MapEntry"));
    assert_eq!(
        BuiltinMember::QueueTryPut.detail(),
        "try_put(value: own T) -> Result[None, SendError[T]]"
    );
    assert_eq!(
        BuiltinMember::TaskGroupStartSoon.detail(),
        "start_soon(function, own ...) -> None"
    );
    assert!(BuiltinMember::TaskResultOrNone
        .docs()
        .contains("Option.None"));
}

#[test]
fn builtin_member_call_shapes_declare_receiver_argument_and_variadic_passing() {
    let owned = [
        (BuiltinMember::VecPush, 0),
        (BuiltinMember::VecSet, 1),
        (BuiltinMember::VecInsert, 1),
        (BuiltinMember::VecExtend, 0),
        (BuiltinMember::MapSet, 0),
        (BuiltinMember::MapSet, 1),
        (BuiltinMember::MapExtend, 0),
        (BuiltinMember::SetInsert, 0),
        (BuiltinMember::QueuePut, 0),
        (BuiltinMember::QueueTryPut, 0),
        (BuiltinMember::QueueGetOr, 0),
        (BuiltinMember::TaskResultOr, 0),
        (BuiltinMember::ProcessSupervisorStart, 0),
        (BuiltinMember::ProcessSupervisorStart, 1),
        (BuiltinMember::ProcessSupervisorStart, 2),
        (BuiltinMember::ProcessSupervisorStart, 3),
        (BuiltinMember::ProcessSupervisorStart, 4),
        (BuiltinMember::ProcessSupervisorStart, 5),
        (BuiltinMember::ProcessSupervisorStart, 6),
        (BuiltinMember::ProcessSupervisorStart, 7),
        (BuiltinMember::ProcessSupervisorStart, 8),
        (BuiltinMember::ProcessSupervisorStart, 9),
        (BuiltinMember::ProcessSupervisorStart, 10),
        (BuiltinMember::HttpExchangeRespondText, 1),
        (BuiltinMember::HttpExchangeRespondText, 2),
        (BuiltinMember::HttpExchangeRespondBytes, 1),
        (BuiltinMember::HttpExchangeRespondBytes, 2),
    ];
    for (member, index) in owned {
        assert_eq!(
            member.argument_passing(index),
            Some(ReceiverKind::Value),
            "{} argument {index} must retain owned metadata",
            member.name()
        );
    }

    for (member, index) in [
        (BuiltinMember::VecContains, 0),
        (BuiltinMember::MapGet, 0),
        (BuiltinMember::MapRemove, 0),
        (BuiltinMember::SetContains, 0),
        (BuiltinMember::SetRemove, 0),
        (BuiltinMember::FileWriteAll, 0),
        (BuiltinMember::ProcessPipeWriteAll, 0),
    ] {
        assert_eq!(
            member.argument_passing(index),
            Some(ReceiverKind::Borrow),
            "{} argument {index} must remain a one-shot shared input",
            member.name()
        );
    }

    assert_eq!(
        BuiltinMember::TaskGroupStart.argument_passing(0),
        Some(ReceiverKind::Borrow)
    );
    assert_eq!(
        BuiltinMember::TaskGroupStart.variadic_argument_passing(),
        Some(ReceiverKind::Value)
    );
    assert_eq!(
        BuiltinMember::VecPush.receiver_passing(),
        ReceiverKind::BorrowMut
    );
    assert_eq!(
        BuiltinMember::VecLen.receiver_passing(),
        ReceiverKind::Borrow
    );
    assert_eq!(
        BuiltinMember::VecGet.argument_passing(0),
        Some(ReceiverKind::Borrow),
        "copy-only slots must preserve the non-consuming diagnostic path"
    );
    assert_eq!(BuiltinMember::VecPush.argument_passing(1), None);
    assert!(BuiltinMember::VecPush.detail().contains("own T"));
    assert!(BuiltinMember::MapSet.detail().contains("own K"));
    assert!(BuiltinMember::QueuePut.detail().contains("own T"));
}

#[test]
fn variadic_task_group_binding_preserves_order_and_rejects_keyword_arguments() {
    let positional = [dummy_arg(None), dummy_arg(None), dummy_arg(None)];
    let bound = BuiltinMember::TaskGroupStartSoon
        .bind_args(&positional, Span::new(1, 1))
        .expect("task-group variadic arguments should bind positionally");
    assert_eq!(bound.len(), positional.len());
    assert!(bound.iter().all(Option::is_some));

    let missing_function = BuiltinMember::TaskGroupStartSoon
        .bind_args(&[], Span::new(4, 9))
        .unwrap_err();
    assert_eq!(
        missing_function.message,
        "`start_soon` is missing required argument `function`"
    );

    let keyword = [dummy_arg(None), dummy_arg(Some("value"))];
    let keyword_error = BuiltinMember::TaskGroupStartSoon
        .bind_args(&keyword, Span::new(1, 1))
        .unwrap_err();
    assert_eq!(
        keyword_error.message,
        "`start_soon` does not take keyword arguments"
    );
    assert_eq!(keyword_error.span, Some(Span::new(1, 1)));
}

#[test]
fn builtin_member_metadata_resolution_and_binding_surface_are_stable() {
    let cases = [
        ("float64", "sqrt", BuiltinMember::FloatSqrt),
        ("int32", "to_string", BuiltinMember::ScalarToString),
        ("int8", "to_string", BuiltinMember::ScalarToString),
        ("int16", "to_string", BuiltinMember::ScalarToString),
        ("int64", "to_string", BuiltinMember::ScalarToString),
        ("int128", "to_string", BuiltinMember::ScalarToString),
        ("intsize", "to_string", BuiltinMember::ScalarToString),
        ("uint8", "to_string", BuiltinMember::ScalarToString),
        ("uint16", "to_string", BuiltinMember::ScalarToString),
        ("uint32", "to_string", BuiltinMember::ScalarToString),
        ("uint64", "to_string", BuiltinMember::ScalarToString),
        ("uint128", "to_string", BuiltinMember::ScalarToString),
        ("uintsize", "to_string", BuiltinMember::ScalarToString),
        ("float32", "to_string", BuiltinMember::ScalarToString),
        ("float64", "to_string", BuiltinMember::ScalarToString),
        ("bool", "to_string", BuiltinMember::ScalarToString),
        ("String", "len", BuiltinMember::StringLen),
        ("String", "byte_len", BuiltinMember::StringByteLen),
        ("String", "contains", BuiltinMember::StringContains),
        ("String", "starts_with", BuiltinMember::StringStartsWith),
        ("String", "ends_with", BuiltinMember::StringEndsWith),
        ("String", "split", BuiltinMember::StringSplit),
        ("String", "replace", BuiltinMember::StringReplace),
        ("String", "to_lower", BuiltinMember::StringToLower),
        ("String", "to_upper", BuiltinMember::StringToUpper),
        ("String", "strip_prefix", BuiltinMember::StringStripPrefix),
        ("String", "strip_suffix", BuiltinMember::StringStripSuffix),
        ("String", "trim", BuiltinMember::StringTrim),
        ("String", "join", BuiltinMember::StringJoin),
        ("String", "clone", BuiltinMember::StringClone),
        ("Vec", "len", BuiltinMember::VecLen),
        ("Vec", "is_empty", BuiltinMember::VecIsEmpty),
        ("Vec", "clone", BuiltinMember::VecClone),
        ("Vec", "push", BuiltinMember::VecPush),
        ("Vec", "pop", BuiltinMember::VecPop),
        ("Vec", "get", BuiltinMember::VecGet),
        ("Vec", "set", BuiltinMember::VecSet),
        ("Vec", "remove", BuiltinMember::VecRemove),
        ("Vec", "swap", BuiltinMember::VecSwap),
        ("Vec", "contains", BuiltinMember::VecContains),
        ("Vec", "extend", BuiltinMember::VecExtend),
        ("Vec", "insert", BuiltinMember::VecInsert),
        ("Vec", "clear", BuiltinMember::VecClear),
        ("Vec", "reverse", BuiltinMember::VecReverse),
        ("Map", "len", BuiltinMember::MapLen),
        ("Map", "is_empty", BuiltinMember::MapIsEmpty),
        ("Map", "clone", BuiltinMember::MapClone),
        ("Map", "get", BuiltinMember::MapGet),
        ("Map", "set", BuiltinMember::MapSet),
        ("Map", "remove", BuiltinMember::MapRemove),
        ("Map", "contains_key", BuiltinMember::MapContainsKey),
        ("Map", "keys", BuiltinMember::MapKeys),
        ("Map", "values", BuiltinMember::MapValues),
        ("Map", "items", BuiltinMember::MapItems),
        ("Map", "entries", BuiltinMember::MapEntries),
        ("Map", "clear", BuiltinMember::MapClear),
        ("Map", "extend", BuiltinMember::MapExtend),
        ("Set", "len", BuiltinMember::SetLen),
        ("Set", "is_empty", BuiltinMember::SetIsEmpty),
        ("Set", "clone", BuiltinMember::SetClone),
        ("Set", "contains", BuiltinMember::SetContains),
        ("Set", "insert", BuiltinMember::SetInsert),
        ("Set", "remove", BuiltinMember::SetRemove),
        ("Queue", "put", BuiltinMember::QueuePut),
        ("Queue", "try_put", BuiltinMember::QueueTryPut),
        ("Queue", "get", BuiltinMember::QueueGet),
        ("Queue", "close", BuiltinMember::QueueClose),
        ("Task", "result", BuiltinMember::TaskResult),
        ("TaskGroup", "start", BuiltinMember::TaskGroupStart),
        ("TaskGroup", "start_soon", BuiltinMember::TaskGroupStartSoon),
        ("TaskGroup", "cancel", BuiltinMember::TaskGroupCancel),
    ];

    for (receiver, name, member) in cases {
        assert_eq!(BuiltinMember::resolve(receiver, name), Some(member));
        assert_eq!(member.name(), name);
        assert!(!member.detail().is_empty());
        assert!(!member.docs().is_empty());
    }
    assert_eq!(BuiltinMember::resolve("Vec", "missing"), None);
    assert_eq!(BuiltinMember::StringLen.detail(), "len() -> int32");
    assert!(BuiltinMember::StringLen.docs().contains("Unicode scalar"));
    assert!(BuiltinMember::StringLen.docs().contains("O(n)"));
    assert_eq!(BuiltinMember::StringByteLen.detail(), "byte_len() -> int32");
    assert!(BuiltinMember::StringByteLen.docs().contains("UTF-8 bytes"));
    assert!(BuiltinMember::StringByteLen.docs().contains("O(1)"));

    let positional_only_error = BuiltinMember::VecReverse
        .bind_args(&[dummy_arg(None)], Span::new(1, 1))
        .unwrap_err();
    assert!(positional_only_error
        .message
        .contains("expects 0 arguments, found 1"));

    let byte_len_error = BuiltinMember::StringByteLen
        .bind_args(&[dummy_arg(None)], Span::new(1, 1))
        .unwrap_err();
    assert!(byte_len_error
        .message
        .contains("expects 0 arguments, found 1"));

    let contains_args_input = [dummy_arg(Some("value"))];
    let contains_args = BuiltinMember::VecContains
        .bind_args(&contains_args_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(contains_args.len(), 1);

    let replace_args_input = [dummy_arg(Some("from")), dummy_arg(Some("to"))];
    let replace_args = BuiltinMember::StringReplace
        .bind_args(&replace_args_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(replace_args.len(), 2);

    let set_error = BuiltinMember::MapSet
        .bind_args(&[dummy_arg(Some("key"))], Span::new(1, 1))
        .unwrap_err();
    assert!(set_error
        .message
        .contains("missing required argument `value`"));

    let send_args_input = [dummy_arg(Some("value"))];
    let send_args = BuiltinMember::QueuePut
        .bind_args(&send_args_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(send_args.len(), 2);
    let try_send_args = BuiltinMember::QueueTryPut
        .bind_args(&send_args_input, Span::new(1, 1))
        .unwrap();
    assert_eq!(try_send_args.len(), 1);

    for member in [
        BuiltinMember::FloatSqrt,
        BuiltinMember::StringLen,
        BuiltinMember::StringByteLen,
        BuiltinMember::StringToLower,
        BuiltinMember::StringToUpper,
        BuiltinMember::StringTrim,
        BuiltinMember::VecLen,
        BuiltinMember::VecIsEmpty,
        BuiltinMember::VecClone,
        BuiltinMember::VecPop,
        BuiltinMember::VecClear,
        BuiltinMember::MapLen,
        BuiltinMember::MapIsEmpty,
        BuiltinMember::MapClone,
        BuiltinMember::MapKeys,
        BuiltinMember::MapValues,
        BuiltinMember::MapItems,
        BuiltinMember::MapEntries,
        BuiltinMember::MapClear,
        BuiltinMember::SetLen,
        BuiltinMember::SetIsEmpty,
        BuiltinMember::SetClone,
        BuiltinMember::StringClone,
        BuiltinMember::QueueClose,
        BuiltinMember::TaskGroupCancel,
    ] {
        let bound = member
            .bind_args(&[], Span::new(1, 1))
            .expect("member should bind");
        assert!(bound.is_empty());
    }

    let task_result_args = BuiltinMember::TaskResult
        .bind_args(&[], Span::new(1, 1))
        .expect("task.result should bind with an optional timeout slot");
    assert_eq!(task_result_args.len(), 1);

    for member in [
        BuiltinMember::StringContains,
        BuiltinMember::StringStartsWith,
        BuiltinMember::StringEndsWith,
        BuiltinMember::StringSplit,
        BuiltinMember::StringStripPrefix,
        BuiltinMember::StringStripSuffix,
        BuiltinMember::StringJoin,
        BuiltinMember::VecPush,
        BuiltinMember::VecGet,
        BuiltinMember::VecRemove,
        BuiltinMember::VecContains,
        BuiltinMember::VecExtend,
        BuiltinMember::MapGet,
        BuiltinMember::MapRemove,
        BuiltinMember::MapContainsKey,
        BuiltinMember::MapExtend,
        BuiltinMember::SetContains,
        BuiltinMember::SetInsert,
        BuiltinMember::SetRemove,
        BuiltinMember::TaskGroupStartSoon,
    ] {
        let args = [dummy_arg(None)];
        let bound = member
            .bind_args(&args, Span::new(1, 1))
            .expect("member should bind");
        assert_eq!(bound.len(), 1);
    }

    for member in [
        BuiltinMember::StringReplace,
        BuiltinMember::VecSet,
        BuiltinMember::VecSwap,
        BuiltinMember::VecInsert,
        BuiltinMember::MapSet,
    ] {
        let args = [dummy_arg(None), dummy_arg(None)];
        let bound = member
            .bind_args(&args, Span::new(1, 1))
            .expect("member should bind");
        assert_eq!(bound.len(), 2);
    }
}

#[test]
fn vec_indexed_mutation_hover_docs_match_runtime_failure_behavior() {
    for member in [BuiltinMember::VecSwap, BuiltinMember::VecInsert] {
        let docs = member.docs();
        assert!(
            docs.contains("runtime error"),
            "{} hover docs should describe out-of-bounds runtime errors: {docs}",
            member.name()
        );
        assert!(
            !docs.contains("returning `false`"),
            "{} hover docs must not promise a false out-of-bounds result: {docs}",
            member.name()
        );
    }
}

#[test]
fn builtin_member_io_network_and_process_metadata_is_stable() {
    let cases = [
        ("fs.File", "read_all", BuiltinMember::FileReadAll),
        ("fs.File", "read_bytes", BuiltinMember::FileReadBytes),
        ("fs.File", "write_all", BuiltinMember::FileWriteAll),
        ("fs.File", "write_bytes", BuiltinMember::FileWriteBytes),
        ("fs.File", "flush", BuiltinMember::FileFlush),
        ("fs.File", "close", BuiltinMember::FileClose),
        (
            "net.TcpListener",
            "accept",
            BuiltinMember::TcpListenerAccept,
        ),
        (
            "net.TcpListener",
            "local_addr",
            BuiltinMember::TcpListenerLocalAddr,
        ),
        ("net.TcpListener", "close", BuiltinMember::TcpListenerClose),
        ("net.TcpStream", "read_all", BuiltinMember::TcpStreamReadAll),
        (
            "net.TcpStream",
            "read_line",
            BuiltinMember::TcpStreamReadLine,
        ),
        (
            "net.TcpStream",
            "read_bytes",
            BuiltinMember::TcpStreamReadBytes,
        ),
        (
            "net.TcpStream",
            "read_exact",
            BuiltinMember::TcpStreamReadExact,
        ),
        (
            "net.TcpStream",
            "write_all",
            BuiltinMember::TcpStreamWriteAll,
        ),
        (
            "net.TcpStream",
            "write_bytes",
            BuiltinMember::TcpStreamWriteBytes,
        ),
        ("net.TcpStream", "flush", BuiltinMember::TcpStreamFlush),
        (
            "net.TcpStream",
            "local_addr",
            BuiltinMember::TcpStreamLocalAddr,
        ),
        (
            "net.TcpStream",
            "peer_addr",
            BuiltinMember::TcpStreamPeerAddr,
        ),
        (
            "net.TcpStream",
            "shutdown_read",
            BuiltinMember::TcpStreamShutdownRead,
        ),
        (
            "net.TcpStream",
            "shutdown_write",
            BuiltinMember::TcpStreamShutdownWrite,
        ),
        (
            "net.TcpStream",
            "shutdown_both",
            BuiltinMember::TcpStreamShutdownBoth,
        ),
        ("net.TcpStream", "close", BuiltinMember::TcpStreamClose),
        (
            "net.UdpSocket",
            "send_text",
            BuiltinMember::UdpSocketSendText,
        ),
        (
            "net.UdpSocket",
            "send_bytes",
            BuiltinMember::UdpSocketSendBytes,
        ),
        ("net.UdpSocket", "recv", BuiltinMember::UdpSocketRecv),
        (
            "net.UdpSocket",
            "recv_from",
            BuiltinMember::UdpSocketRecvFrom,
        ),
        (
            "net.UdpSocket",
            "local_addr",
            BuiltinMember::UdpSocketLocalAddr,
        ),
        (
            "net.UdpSocket",
            "peer_addr",
            BuiltinMember::UdpSocketPeerAddr,
        ),
        ("net.UdpSocket", "close", BuiltinMember::UdpSocketClose),
        (
            "net.UdpDatagram",
            "address",
            BuiltinMember::UdpDatagramAddress,
        ),
        ("net.UdpDatagram", "bytes", BuiltinMember::UdpDatagramBytes),
        ("net.UdpDatagram", "text", BuiltinMember::UdpDatagramText),
        (
            "net.HttpListener",
            "accept",
            BuiltinMember::HttpListenerAccept,
        ),
        (
            "net.HttpListener",
            "local_addr",
            BuiltinMember::HttpListenerLocalAddr,
        ),
        (
            "net.HttpListener",
            "close",
            BuiltinMember::HttpListenerClose,
        ),
        (
            "net.HttpExchange",
            "method",
            BuiltinMember::HttpExchangeMethod,
        ),
        ("net.HttpExchange", "path", BuiltinMember::HttpExchangePath),
        (
            "net.HttpExchange",
            "headers",
            BuiltinMember::HttpExchangeHeaders,
        ),
        (
            "net.HttpExchange",
            "body_text",
            BuiltinMember::HttpExchangeBodyText,
        ),
        (
            "net.HttpExchange",
            "body_bytes",
            BuiltinMember::HttpExchangeBodyBytes,
        ),
        (
            "net.HttpExchange",
            "respond_text",
            BuiltinMember::HttpExchangeRespondText,
        ),
        (
            "net.HttpExchange",
            "respond_bytes",
            BuiltinMember::HttpExchangeRespondBytes,
        ),
        (
            "net.HttpResponse",
            "status",
            BuiltinMember::HttpResponseStatus,
        ),
        (
            "net.HttpResponse",
            "reason",
            BuiltinMember::HttpResponseReason,
        ),
        (
            "net.HttpResponse",
            "headers",
            BuiltinMember::HttpResponseHeaders,
        ),
        ("net.HttpResponse", "text", BuiltinMember::HttpResponseText),
        (
            "net.HttpResponse",
            "bytes",
            BuiltinMember::HttpResponseBytes,
        ),
        (
            "net.WebSocketListener",
            "accept",
            BuiltinMember::WebSocketListenerAccept,
        ),
        (
            "net.WebSocketListener",
            "local_addr",
            BuiltinMember::WebSocketListenerLocalAddr,
        ),
        (
            "net.WebSocket",
            "send_text",
            BuiltinMember::WebSocketSendText,
        ),
        (
            "net.WebSocket",
            "send_bytes",
            BuiltinMember::WebSocketSendBytes,
        ),
        (
            "net.WebSocket",
            "recv_text",
            BuiltinMember::WebSocketRecvText,
        ),
        (
            "net.WebSocket",
            "recv_bytes",
            BuiltinMember::WebSocketRecvBytes,
        ),
        ("net.WebSocket", "close", BuiltinMember::WebSocketClose),
        (
            "net.UnixListener",
            "accept",
            BuiltinMember::UnixListenerAccept,
        ),
        (
            "net.UnixListener",
            "close",
            BuiltinMember::UnixListenerClose,
        ),
        (
            "net.UnixStream",
            "read_line",
            BuiltinMember::UnixStreamReadLine,
        ),
        (
            "net.UnixStream",
            "read_exact",
            BuiltinMember::UnixStreamReadExact,
        ),
        (
            "net.UnixStream",
            "write_all",
            BuiltinMember::UnixStreamWriteAll,
        ),
        ("net.UnixStream", "close", BuiltinMember::UnixStreamClose),
        (
            "net.TlsListener",
            "accept",
            BuiltinMember::TlsListenerAccept,
        ),
        (
            "net.TlsListener",
            "local_addr",
            BuiltinMember::TlsListenerLocalAddr,
        ),
        ("net.TlsListener", "close", BuiltinMember::TlsListenerClose),
        (
            "net.TlsStream",
            "read_line",
            BuiltinMember::TlsStreamReadLine,
        ),
        (
            "net.TlsStream",
            "read_exact",
            BuiltinMember::TlsStreamReadExact,
        ),
        (
            "net.TlsStream",
            "write_all",
            BuiltinMember::TlsStreamWriteAll,
        ),
        ("net.TlsStream", "close", BuiltinMember::TlsStreamClose),
        ("process.Child", "stdin", BuiltinMember::ProcessChildStdin),
        ("process.Child", "stdout", BuiltinMember::ProcessChildStdout),
        ("process.Child", "stderr", BuiltinMember::ProcessChildStderr),
        ("process.Child", "wait", BuiltinMember::ProcessChildWait),
        (
            "process.Child",
            "wait_or_none",
            BuiltinMember::ProcessChildWaitOrNone,
        ),
        (
            "process.Child",
            "wait_ok",
            BuiltinMember::ProcessChildWaitOk,
        ),
        ("process.Child", "kill", BuiltinMember::ProcessChildKill),
        (
            "process.Child",
            "terminate",
            BuiltinMember::ProcessChildTerminate,
        ),
        ("process.Child", "close", BuiltinMember::ProcessChildClose),
        (
            "process.Pipe",
            "read_all",
            BuiltinMember::ProcessPipeReadAll,
        ),
        (
            "process.Pipe",
            "read_line",
            BuiltinMember::ProcessPipeReadLine,
        ),
        (
            "process.Pipe",
            "read_bytes",
            BuiltinMember::ProcessPipeReadBytes,
        ),
        (
            "process.Pipe",
            "write_all",
            BuiltinMember::ProcessPipeWriteAll,
        ),
        (
            "process.Pipe",
            "write_bytes",
            BuiltinMember::ProcessPipeWriteBytes,
        ),
        ("process.Pipe", "flush", BuiltinMember::ProcessPipeFlush),
        ("process.Pipe", "close", BuiltinMember::ProcessPipeClose),
        (
            "process.Completed",
            "status",
            BuiltinMember::ProcessCompletedStatus,
        ),
        (
            "process.Completed",
            "success",
            BuiltinMember::ProcessCompletedSuccess,
        ),
        (
            "process.Completed",
            "stdout",
            BuiltinMember::ProcessCompletedStdout,
        ),
        (
            "process.Completed",
            "stdout_bytes",
            BuiltinMember::ProcessCompletedStdoutBytes,
        ),
        (
            "process.Completed",
            "stderr",
            BuiltinMember::ProcessCompletedStderr,
        ),
        (
            "process.Completed",
            "stderr_bytes",
            BuiltinMember::ProcessCompletedStderrBytes,
        ),
        (
            "process.Completed",
            "check",
            BuiltinMember::ProcessCompletedCheck,
        ),
        (
            "process.Supervisor",
            "start",
            BuiltinMember::ProcessSupervisorStart,
        ),
        (
            "process.Supervisor",
            "wait",
            BuiltinMember::ProcessSupervisorWait,
        ),
        (
            "process.Supervisor",
            "wait_or_none",
            BuiltinMember::ProcessSupervisorWaitOrNone,
        ),
        (
            "process.Supervisor",
            "stop",
            BuiltinMember::ProcessSupervisorStop,
        ),
        (
            "process.Supervisor",
            "is_empty",
            BuiltinMember::ProcessSupervisorIsEmpty,
        ),
        (
            "process.Supervisor",
            "close",
            BuiltinMember::ProcessSupervisorClose,
        ),
    ];

    for (receiver, name, member) in cases {
        assert_eq!(BuiltinMember::resolve(receiver, name), Some(member));
        assert_eq!(member.name(), name);
        assert!(!member.detail().is_empty());
        assert!(!member.docs().is_empty());
    }
}

#[test]
fn builtin_member_io_network_and_process_bindings_cover_argument_shapes() {
    for member in [
        BuiltinMember::FileReadAll,
        BuiltinMember::FileReadBytes,
        BuiltinMember::FileFlush,
        BuiltinMember::FileClose,
        BuiltinMember::TcpListenerLocalAddr,
        BuiltinMember::TcpListenerClose,
        BuiltinMember::TcpStreamFlush,
        BuiltinMember::TcpStreamLocalAddr,
        BuiltinMember::TcpStreamPeerAddr,
        BuiltinMember::TcpStreamShutdownRead,
        BuiltinMember::TcpStreamShutdownWrite,
        BuiltinMember::TcpStreamShutdownBoth,
        BuiltinMember::TcpStreamClose,
        BuiltinMember::UdpSocketLocalAddr,
        BuiltinMember::UdpSocketPeerAddr,
        BuiltinMember::UdpSocketClose,
        BuiltinMember::UdpDatagramAddress,
        BuiltinMember::UdpDatagramBytes,
        BuiltinMember::UdpDatagramText,
        BuiltinMember::HttpListenerLocalAddr,
        BuiltinMember::HttpListenerClose,
        BuiltinMember::HttpExchangeMethod,
        BuiltinMember::HttpExchangePath,
        BuiltinMember::HttpExchangeHeaders,
        BuiltinMember::HttpExchangeBodyText,
        BuiltinMember::HttpExchangeBodyBytes,
        BuiltinMember::HttpResponseStatus,
        BuiltinMember::HttpResponseReason,
        BuiltinMember::HttpResponseHeaders,
        BuiltinMember::HttpResponseText,
        BuiltinMember::HttpResponseBytes,
        BuiltinMember::WebSocketListenerLocalAddr,
        BuiltinMember::WebSocketClose,
        BuiltinMember::UnixListenerClose,
        BuiltinMember::UnixStreamClose,
        BuiltinMember::TlsListenerLocalAddr,
        BuiltinMember::TlsListenerClose,
        BuiltinMember::TlsStreamClose,
        BuiltinMember::ProcessChildStdin,
        BuiltinMember::ProcessChildStdout,
        BuiltinMember::ProcessChildStderr,
        BuiltinMember::ProcessChildKill,
        BuiltinMember::ProcessChildTerminate,
        BuiltinMember::ProcessChildClose,
        BuiltinMember::ProcessPipeReadAll,
        BuiltinMember::ProcessPipeFlush,
        BuiltinMember::ProcessPipeClose,
        BuiltinMember::ProcessCompletedStatus,
        BuiltinMember::ProcessCompletedSuccess,
        BuiltinMember::ProcessCompletedStdout,
        BuiltinMember::ProcessCompletedStdoutBytes,
        BuiltinMember::ProcessCompletedStderr,
        BuiltinMember::ProcessCompletedStderrBytes,
        BuiltinMember::ProcessCompletedCheck,
        BuiltinMember::ProcessSupervisorStop,
        BuiltinMember::ProcessSupervisorIsEmpty,
        BuiltinMember::ProcessSupervisorClose,
    ] {
        let bound = member
            .bind_args(&[], Span::new(1, 1))
            .expect("zero-argument member should bind");
        assert!(bound.is_empty());
    }

    for member in [
        BuiltinMember::TcpListenerAccept,
        BuiltinMember::TcpStreamReadAll,
        BuiltinMember::TcpStreamReadLine,
        BuiltinMember::WebSocketRecvText,
        BuiltinMember::WebSocketRecvBytes,
        BuiltinMember::WebSocketListenerAccept,
        BuiltinMember::HttpListenerAccept,
        BuiltinMember::UnixListenerAccept,
        BuiltinMember::UnixStreamReadLine,
        BuiltinMember::TlsListenerAccept,
        BuiltinMember::TlsStreamReadLine,
        BuiltinMember::ProcessChildWait,
        BuiltinMember::ProcessChildWaitOrNone,
        BuiltinMember::ProcessChildWaitOk,
        BuiltinMember::ProcessPipeReadLine,
        BuiltinMember::ProcessSupervisorWait,
        BuiltinMember::ProcessSupervisorWaitOrNone,
    ] {
        let bound = member
            .bind_args(&[], Span::new(1, 1))
            .expect("timeout-only member should bind with omitted timeout");
        assert_eq!(bound.len(), 1);
    }

    for member in [BuiltinMember::FileWriteAll, BuiltinMember::FileWriteBytes] {
        let args = [dummy_arg(None)];
        let bound = member
            .bind_args(&args, Span::new(1, 1))
            .expect("single-argument file member should bind");
        assert_eq!(bound.len(), 1);
    }

    for member in [
        BuiltinMember::TcpStreamWriteAll,
        BuiltinMember::UnixStreamWriteAll,
        BuiltinMember::TlsStreamWriteAll,
        BuiltinMember::ProcessPipeWriteAll,
        BuiltinMember::WebSocketSendText,
        BuiltinMember::TcpStreamWriteBytes,
        BuiltinMember::WebSocketSendBytes,
        BuiltinMember::ProcessPipeWriteBytes,
        BuiltinMember::TcpStreamReadBytes,
        BuiltinMember::UdpSocketRecv,
        BuiltinMember::UdpSocketRecvFrom,
        BuiltinMember::ProcessPipeReadBytes,
        BuiltinMember::TcpStreamReadExact,
        BuiltinMember::UnixStreamReadExact,
        BuiltinMember::TlsStreamReadExact,
    ] {
        let args = [dummy_arg(None)];
        let bound = member
            .bind_args(&args, Span::new(1, 1))
            .expect("single-required-plus-timeout member should bind");
        assert_eq!(bound.len(), 2);
    }

    for member in [
        BuiltinMember::UdpSocketSendText,
        BuiltinMember::UdpSocketSendBytes,
    ] {
        let args = [dummy_arg(None), dummy_arg(None)];
        let bound = member
            .bind_args(&args, Span::new(1, 1))
            .expect("address/value UDP member should bind");
        assert_eq!(bound.len(), 3);
    }

    for member in [
        BuiltinMember::HttpExchangeRespondText,
        BuiltinMember::HttpExchangeRespondBytes,
    ] {
        let args = [dummy_arg(None), dummy_arg(None), dummy_arg(None)];
        let bound = member
            .bind_args(&args, Span::new(1, 1))
            .expect("HTTP response member should bind");
        assert_eq!(bound.len(), 3);
    }

    let supervisor_args = [dummy_arg(None), dummy_arg(None)];
    let supervisor_start = BuiltinMember::ProcessSupervisorStart
        .bind_args(&supervisor_args, Span::new(1, 1))
        .expect("supervisor start should bind with optional process settings omitted");
    assert_eq!(supervisor_start.len(), 11);
}

#[test]
fn concurrency_builtin_surface_uses_structured_wait_helpers_only() {
    assert_eq!(BuiltinFunction::from_name("queue"), None);
    assert_eq!(BuiltinFunction::from_name("tasks"), None);
    assert_eq!(BuiltinFunction::from_name("after"), None);
    assert_eq!(
        BuiltinFunction::from_name("wait_any"),
        Some(BuiltinFunction::WaitAny)
    );
    assert_eq!(
        BuiltinFunction::from_name("wait_all"),
        Some(BuiltinFunction::WaitAll)
    );

    assert_eq!(
        BuiltinMember::resolve("Queue", "try_put"),
        Some(BuiltinMember::QueueTryPut)
    );
    assert_eq!(
        BuiltinMember::resolve("TaskGroup", "start_soon"),
        Some(BuiltinMember::TaskGroupStartSoon)
    );
    assert_eq!(
        BuiltinMember::QueueGet.detail(),
        "get(timeout: Duration = ...) -> QueueReceive[T]"
    );
    assert_eq!(
        BuiltinMember::TaskResult.detail(),
        "result(timeout: Duration = ...) -> TaskResult[T]"
    );
    assert_eq!(
        BuiltinMember::TaskGroupStartSoon.detail(),
        "start_soon(function, own ...) -> None"
    );
    assert!(BuiltinFunction::WaitAny
        .docs()
        .contains("first task to complete"));
    assert!(BuiltinFunction::WaitAll.docs().contains("every task"));
}
