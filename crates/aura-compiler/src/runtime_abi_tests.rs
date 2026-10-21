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
    use crate::runtime_ops::RuntimeOp;

    let mut covered = BTreeSet::new();
    for op in RuntimeOp::all() {
        let call = op.runtime_call();
        assert!(
            RuntimeFn::ALL.contains(&call.function),
            "{op} calls undeclared helper {:?}",
            call.function
        );
        let words = usize::from(op.has_receiver())
            + op.params().count()
            + call.immediates.len()
            + 2 * usize::from(call.span);
        assert_eq!(
            words,
            call.function.params().len(),
            "{op} passes {words} words to `{}`",
            call.function.symbol()
        );
        covered.insert(call.function);
    }
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
}

#[test]
fn every_runtime_op_round_trips_through_its_builtin_member() {
    use crate::runtime_ops::RuntimeOp;

    for op in RuntimeOp::all() {
        assert_eq!(
            RuntimeOp::from_builtin_member(op.builtin_member()),
            Some(op),
            "{op} does not resolve back from `{}`",
            op.member_name()
        );
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(serde_json::from_str::<RuntimeOp>(&json).unwrap(), op);
    }
}
