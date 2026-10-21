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
