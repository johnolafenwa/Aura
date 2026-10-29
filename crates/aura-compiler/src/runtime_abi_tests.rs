use std::collections::BTreeSet;

use super::*;

#[test]
fn runtime_helpers_have_unique_direct_symbols() {
    let mut symbols = BTreeSet::new();
    for function in RuntimeFn::ALL {
        assert!(
            function.symbol().starts_with("aura_direct_"),
            "{function:?} names `{}` outside the direct runtime",
            function.symbol()
        );
        assert!(
            symbols.insert(function.symbol()),
            "{function:?} repeats `{}`",
            function.symbol()
        );
    }
}

#[test]
fn runtime_tables_index_each_helper_in_declaration_order() {
    let table = RuntimeTable::try_build(|function| Ok::<_, ()>(function.symbol())).unwrap();
    for function in RuntimeFn::ALL {
        assert_eq!(table[*function], function.symbol());
    }
    let lengths = table.map(|symbol| symbol.len());
    assert_eq!(
        lengths[RuntimeFn::ReleaseValue],
        "aura_direct_release_value".len()
    );
    assert_eq!(RuntimeFn::PrintF64.params(), &[AbiType::F64]);
    assert_eq!(RuntimeFn::RunRoot.result(), Some(AbiType::I32));
}

#[test]
fn every_runtime_op_calls_a_declared_helper_with_matching_arity() {
    use crate::runtime_ops::{ListOp, RuntimeOp, RuntimeTarget};

    let mut covered = BTreeSet::new();
    let mut host_symbols = BTreeSet::new();
    for op in RuntimeOp::all() {
        let call = match op.runtime_target() {
            RuntimeTarget::Helper(call) => call,
            RuntimeTarget::Host(symbol) => {
                assert!(matches!(op, RuntimeOp::Host(_)), "{op} calls a host entry");
                assert!(symbol.starts_with("aura_host_"), "{op} calls `{symbol}`");
                assert!(host_symbols.insert(symbol), "{op} repeats `{symbol}`");
                continue;
            }
        };
        assert_eq!(op.runtime_call(), call);
        assert!(
            RuntimeFn::ALL.contains(&call.function),
            "{op} calls undeclared helper {:?}",
            call.function
        );
        // A scalar list read's line and column reach only its bounds
        // failure's call, not the helper.
        let unpassed = 2 * usize::from(matches!(
            op,
            RuntimeOp::List(ListOp::IndexScalar(_) | ListOp::IndexScalarUnchecked(_))
        ));
        let words = call.layout_words
            + usize::from(op.has_receiver())
            + op.params().len()
            + call.immediates.len()
            + 2 * usize::from(call.span)
            - unpassed;
        assert_eq!(
            words,
            call.function.params().len(),
            "{op} passes {words} words to `{}`",
            call.function.symbol()
        );
        covered.insert(call.function);
    }
    // Every host table entry has an op, except the ones a dedicated
    // helper replaces.
    let replaced = crate::native_runtime::HOST_BUILTIN_SYMBOLS
        .iter()
        .filter(|(name, _)| crate::runtime_ops::FunctionOp::from_name(name).is_some())
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    assert_eq!(replaced, ["sys::monotonic_time_ms"]);
    assert_eq!(
        host_symbols.len() + replaced.len(),
        crate::native_runtime::HOST_BUILTIN_SYMBOLS.len()
    );
    // Each resource handle method has its own helper.
    let resource_ops = RuntimeOp::all()
        .filter(|op| matches!(op, RuntimeOp::Resource(_)))
        .count();
    let resource_helpers = RuntimeOp::all()
        .filter(|op| matches!(op, RuntimeOp::Resource(_)))
        .map(|op| op.runtime_call().function)
        .collect::<BTreeSet<_>>();
    assert_eq!(resource_ops, resource_helpers.len());
    assert!(covered.contains(&RuntimeFn::IntegerWidthBinary));
    // Every helper a named built-in call, `print`, or a `Duration`
    // constructor calls has an op.
    let named_call_helpers = [
        RuntimeFn::PrintI64,
        RuntimeFn::PrintU64,
        RuntimeFn::PrintF32,
        RuntimeFn::PrintF64,
        RuntimeFn::PrintBool,
        RuntimeFn::PrintValue,
        RuntimeFn::RngNew,
        RuntimeFn::RandomSecureInt,
        RuntimeFn::RandomSecureBytes,
        RuntimeFn::ArrayZeros,
        RuntimeFn::ArrayFull,
        RuntimeFn::ArrayFromVec,
        RuntimeFn::DurationFromI64,
        RuntimeFn::CollectionOperation,
        RuntimeFn::VecEmpty,
        RuntimeFn::MapEmpty,
        RuntimeFn::SetEmpty,
        RuntimeFn::RangeNew,
        RuntimeFn::ChannelNew,
        RuntimeFn::TaskGroupNew,
        RuntimeFn::Cancelled,
        RuntimeFn::YieldNow,
        RuntimeFn::SleepValueVoid,
        RuntimeFn::Select,
        RuntimeFn::WaitAny,
        RuntimeFn::WaitAnyTimeoutValue,
        RuntimeFn::WaitAll,
        RuntimeFn::WaitAllTimeoutValue,
        RuntimeFn::AbsValue,
        RuntimeFn::MinValue,
        RuntimeFn::MaxValue,
        RuntimeFn::SqrtValue,
        RuntimeFn::RoundValue,
        RuntimeFn::DivmodValue,
        RuntimeFn::ParseInt32,
        RuntimeFn::ParseInt64,
        RuntimeFn::ParseFloat64,
        RuntimeFn::MonotonicTimeMs,
        RuntimeFn::IoWrite,
        RuntimeFn::IoFlush,
        RuntimeFn::IoReadLine,
        RuntimeFn::ProcessInherit,
        RuntimeFn::ProcessNull,
        RuntimeFn::ProcessPipe,
        RuntimeFn::ProcessSupervisor,
        RuntimeFn::ProcessStart,
        RuntimeFn::ProcessRun,
    ];
    for helper in named_call_helpers
        .into_iter()
        .chain(RuntimeFn::ALL.iter().copied().filter(|helper| {
            helper.symbol().starts_with("aura_direct_fs_")
                || helper.symbol().starts_with("aura_direct_net_")
        }))
    {
        assert!(covered.contains(&helper), "{helper:?} has no runtime op");
    }
    // Every `str`, queue, and task helper a member call makes has an op.
    // Slicing, `with` cleanup of a task group, and task start stay in the
    // backend, as do the tests and replacements with literal arguments,
    // copies read out of a list's `str` view, and the forms of `get` and
    // `put` a receive slot or a discarded `put` calls.
    for helper in RuntimeFn::ALL.iter().copied().filter(|helper| {
        let symbol = helper.symbol();
        (symbol.starts_with("aura_direct_string_")
            || symbol.starts_with("aura_direct_channel_")
            || symbol.starts_with("aura_direct_task_"))
            && !matches!(
                helper,
                RuntimeFn::StringLiteral
                    | RuntimeFn::StringSlice
                    | RuntimeFn::StringAffixLiteral
                    | RuntimeFn::StringReplaceLiteral
                    | RuntimeFn::StringFromView
                    | RuntimeFn::TaskGroupClose
                    | RuntimeFn::TaskArgBufferGuard
                    | RuntimeFn::TaskArgBufferDisarm
                    | RuntimeFn::ChannelSendDiscarded
                    | RuntimeFn::ChannelRecvItem
                    | RuntimeFn::ChannelRecvInTaskGroupItem
                    | RuntimeFn::ChannelRecvWithRegisteredProducersItem
            )
    }) {
        assert!(covered.contains(&helper), "{helper:?} has no runtime op");
    }
    // Every `list`, `dict`, `set`, and `Array` helper a member call makes
    // has an op. The shared collection operation pops and removes, an
    // element path checks its index, and a lent element's `len` and `get`
    // are read in place. A plain record element is read, appended,
    // replaced, and popped in place. An empty list literal of a known type
    // is built and tagged in one call, its `str` literals join it without
    // boxes, and a `str` element copied out of a
    // list that keeps no view is read by index. A `get` whose `Lookup` is kept as its
    // found value calls the `_payload` form of the op's helper, and one
    // whose found value is only read calls the `_lent` form. A `contains_key`
    // test that an entry loan follows probes the entry for both, and an
    // `int64` dict value is stored from its word.
    let uncovered = RuntimeFn::ALL
        .iter()
        .copied()
        .filter(|helper| {
            let symbol = helper.symbol();
            [
                "aura_direct_vec_",
                "aura_direct_map_",
                "aura_direct_set_",
                "aura_direct_array_",
            ]
            .iter()
            .any(|prefix| symbol.starts_with(prefix))
                && !covered.contains(helper)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        uncovered,
        [
            RuntimeFn::SetReturnedViewProjection,
            RuntimeFn::SetNextMutableSinks,
            RuntimeFn::SetNextIndirectMutableSinks,
            RuntimeFn::VecStrElement,
            RuntimeFn::VecEmptyTagged,
            RuntimeFn::VecPushStrLiteral,
            RuntimeFn::VecLend,
            RuntimeFn::VecPopScalar,
            RuntimeFn::VecCheckIndex,
            RuntimeFn::VecReadPlain,
            RuntimeFn::VecPushPlain,
            RuntimeFn::VecSetPlain,
            RuntimeFn::VecPopPlain,
            RuntimeFn::VecElementLen,
            RuntimeFn::VecElementMapGet,
            RuntimeFn::VecElementMapGetPayload,
            RuntimeFn::VecElementMapGetStr,
            RuntimeFn::VecElementMapGetStrPayload,
            RuntimeFn::VecElementMapGetLent,
            RuntimeFn::VecElementMapGetStrLent,
            RuntimeFn::MapGetPayload,
            RuntimeFn::MapGetStrPayload,
            RuntimeFn::MapGetInt64Payload,
            RuntimeFn::MapGetLent,
            RuntimeFn::MapGetStrLent,
            RuntimeFn::MapValueSlot,
            RuntimeFn::MapValueSlotInt64,
            RuntimeFn::MapProbeSlot,
            RuntimeFn::MapProbeSlotInt64,
            RuntimeFn::MapGetInt64Lent,
            RuntimeFn::MapSetIndexInt64,
        ]
    );
}

#[test]
fn every_runtime_op_round_trips_through_its_builtin_member_or_name() {
    use crate::runtime_ops::{builtin_function_for_name, RuntimeOp};

    for op in RuntimeOp::all() {
        match op.builtin_member() {
            Some(member) => assert_eq!(
                RuntimeOp::from_builtin_member(member),
                Some(op.unspecialized()),
                "{op} does not resolve back from `{}`",
                op.name()
            ),
            // A private member resolves by its MIR spelling.
            None if op.has_receiver() => assert_eq!(
                RuntimeOp::resolve_member(
                    &crate::sema::Type::named(private_receiver(op)),
                    op.name(),
                    |_| false
                ),
                Some(op.unspecialized()),
                "{op} does not resolve back from `{}`",
                op.name()
            ),
            None => assert_eq!(
                builtin_function_for_name(op.name(), |_| false),
                Some(op.unspecialized()),
                "{op} does not resolve back from `{}`",
                op.name()
            ),
        }
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(serde_json::from_str::<RuntimeOp>(&json).unwrap(), op);
    }
}

/// The receiver type a private member op is spelled on.
fn private_receiver(op: crate::runtime_ops::RuntimeOp) -> &'static str {
    use crate::runtime_ops::RuntimeOp;
    match op {
        RuntimeOp::Queue(_) => "Queue",
        RuntimeOp::Str(_) => "str",
        RuntimeOp::Dict(_) => "dict",
        RuntimeOp::Set(_) => "set",
        RuntimeOp::Array(_) => "Array",
        _ => "list",
    }
}

#[test]
fn only_the_frame_stack_accessor_is_task_invariant() {
    let invariant = RuntimeFn::ALL
        .iter()
        .filter(|function| function.is_task_invariant())
        .collect::<Vec<_>>();
    assert_eq!(invariant, [&RuntimeFn::FrameStack]);
    assert_eq!(RuntimeFn::FrameStack.params(), &[] as &[AbiType]);
    assert_eq!(RuntimeFn::FrameStack.result(), Some(AbiType::I64));
}

#[test]
fn the_frame_stack_layout_comes_from_the_runtime() {
    let layout = frame_stack_layout();
    assert_eq!(
        layout,
        aura_runtime::native_runtime::direct_frame_stack_layout()
    );
    assert_eq!(FRAME_STACK_ENTRY_BYTES, 8);
}
