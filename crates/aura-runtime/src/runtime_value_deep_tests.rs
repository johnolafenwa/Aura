//! Values nested as deep as a program builds them: a long chain of variants
//! copies, compares, and drops without walking the call stack.

use super::*;

/// The chain `Cons(depth - 1, Cons(depth - 2, ... Nil))`.
fn chain(depth: i64) -> Value {
    let mut value = Value::EnumVariant(EnumVariantValue {
        enum_name: RuntimeName::from_static("Chain"),
        variant_name: RuntimeName::from_static("Nil"),
        payloads: VariantPayloads::Values(Vec::new()),
    });
    for index in 0..depth {
        value = Value::EnumVariant(EnumVariantValue {
            enum_name: RuntimeName::from_static("Chain"),
            variant_name: RuntimeName::from_static("Cons"),
            payloads: VariantPayloads::Values(vec![
                Value::Int(IntegerValue::from_i64(index)),
                value,
            ]),
        });
    }
    value
}

/// Runs `work` on a thread with a stack far smaller than the chains.
fn on_small_stack(work: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(work)
        .expect("the thread starts")
        .join()
        .expect("the work finishes without overflowing its stack");
}

#[test]
fn deep_variant_chain_drops_without_recursion() {
    on_small_stack(|| drop(chain(200_000)));
}

#[test]
fn deep_variant_chains_compare_without_recursion() {
    on_small_stack(|| {
        let (left, right) = (chain(200_000), chain(200_000));
        assert!(left == right);
        let other = chain(199_999);
        assert!(left != other);
    });
}

#[test]
fn deep_variant_chain_copies_without_recursion() {
    on_small_stack(|| {
        let original = chain(200_000);
        let copy = original.clone();
        assert!(copy == original);
        drop(original);
        // The copy owns its own payloads.
        let Value::EnumVariant(variant) = &copy else {
            panic!("a variant");
        };
        assert_eq!(variant.variant_name.as_str(), "Cons");
        assert_eq!(variant.payloads.len(), 2);
    });
}

#[test]
fn shallow_variants_compare_by_their_payloads() {
    let one = chain(3);
    let two = chain(3);
    assert_eq!(one, two);
    let Value::EnumVariant(mut changed) = chain(3) else {
        panic!("a variant");
    };
    changed.payloads.values_mut()[0] = Value::Int(IntegerValue::from_i64(9));
    assert_ne!(one, Value::EnumVariant(changed));
}
