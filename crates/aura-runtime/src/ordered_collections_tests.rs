use super::*;

fn number(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn call(function: host_builtins::HostBuiltinFn, args: Vec<Value>) -> Value {
    function(args, None).expect("valid collection operation")
}

fn split(value: Value) -> (Value, Value) {
    let Value::Tuple(tuple) = value else {
        panic!("expected a pair")
    };
    let [left, right]: [Value; 2] = tuple.elements.try_into().expect("two elements");
    (left, right)
}

#[test]
fn deque_preserves_snapshots_order_and_bounds() {
    let empty = call(new, vec![number(0)]);
    let a = call(
        deque_push,
        vec![empty.clone(), Value::string("a"), Value::Bool(true)],
    );
    let ab = call(
        deque_push,
        vec![a.clone(), Value::string("b"), Value::Bool(false)],
    );
    assert_eq!(call(len, vec![empty.clone()]), number(0));
    assert_eq!(call(len, vec![a]), number(1));
    for (index, expected) in [
        (0, "Lookup.Found(a)"),
        (-1, "Lookup.Found(b)"),
        (-3, "Lookup.Missing"),
        (2, "Lookup.Missing"),
    ] {
        assert_eq!(
            call(deque_get, vec![ab.clone(), number(index)]).render(),
            expected
        );
    }
    let (b, first) = split(call(deque_pop, vec![ab, Value::Bool(true)]));
    assert_eq!(first, Value::string("a"));
    let (drained, last) = split(call(deque_pop, vec![b, Value::Bool(false)]));
    assert_eq!(last, Value::string("b"));
    assert_eq!(call(len, vec![drained]), number(0));
    assert_eq!(
        deque_pop(vec![empty, Value::Bool(false)], None)
            .unwrap_err()
            .code,
        "AU4003"
    );
}

#[test]
fn heaps_order_integers_strings_and_nan() {
    let min = call(heap_from, vec![list(vec![number(4), number(1), number(3)])]);
    assert_eq!(
        call(heap_peek, vec![min.clone()]).render(),
        "Lookup.Found(1)"
    );
    let (_, first) = split(call(heap_take, vec![min]));
    assert_eq!(first, number(1));
    let empty = call(new, vec![number(2)]);
    let max = call(heap_push, vec![empty.clone(), Value::string("m")]);
    let max = call(heap_push, vec![max, Value::string("z")]);
    let (max, first) = split(call(heap_take, vec![max]));
    assert_eq!(first, Value::string("z"));
    assert_eq!(call(heap_peek, vec![max]).render(), "Lookup.Found(m)");
    assert_eq!(heap_take(vec![empty], None).unwrap_err().code, "AU4003");
    let min = call(new, vec![number(1)]);
    let min = call(heap_push, vec![min, Value::Float(f64::NAN)]);
    let min = call(heap_push, vec![min, Value::Float(1.5)]);
    let (_, first) = split(call(heap_take, vec![min]));
    assert_eq!(first, Value::Float(1.5));
    assert_eq!(call(supported, vec![Value::Unit]), Value::Bool(false));
    assert_eq!(call(supported, vec![number(5)]), Value::Bool(true));
}

#[test]
fn btree_lookup_range_drain_and_cow() {
    let empty = call(new, vec![number(3)]);
    let first = call(map_put, vec![empty.clone(), number(2), Value::string("b")]);
    let map = call(map_put, vec![first.clone(), number(6), Value::string("f")]);
    let map = call(map_put, vec![map, number(4), Value::string("d")]);
    assert_eq!(
        call(map_get, vec![map.clone(), number(4)]).render(),
        "Lookup.Found(d)"
    );
    assert_eq!(
        call(map_has, vec![first.clone(), number(4)]),
        Value::Bool(false)
    );
    assert_eq!(call(len, vec![map.clone()]), number(3));
    assert_eq!(
        call(map_edge, vec![map.clone(), Value::Bool(true)]).render(),
        "Lookup.Found((6, f))"
    );
    assert_eq!(
        call(map_edge, vec![map.clone(), Value::Bool(false)]).render(),
        "Lookup.Found((2, b))"
    );
    for (key, mode, expected) in [
        (5, 0, "Lookup.Found((4, d))"),
        (5, 1, "Lookup.Found((6, f))"),
        (5, 2, "Lookup.Missing"),
        (4, 2, "Lookup.Found((4, d))"),
        (1, 0, "Lookup.Missing"),
        (9, 1, "Lookup.Missing"),
    ] {
        assert_eq!(
            call(map_bound, vec![map.clone(), number(key), number(mode)]).render(),
            expected
        );
    }
    assert_eq!(
        call(map_range, vec![map.clone(), number(2), number(6)]).render(),
        "[(2, b), (4, d)]"
    );
    assert_eq!(
        call(map_range, vec![map.clone(), number(6), number(2)]).render(),
        "[]"
    );
    let (map, removed) = split(call(map_take, vec![map, number(4)]));
    assert_eq!(removed.render(), "Lookup.Found(d)");
    assert_eq!(call(map_drain, vec![map]).render(), "[(2, b), (6, f)]");
    assert_eq!(call(map_drain, vec![first]).render(), "[(2, b)]");
    assert_eq!(call(map_drain, vec![empty]).render(), "[]");
}

#[test]
fn operations_reject_wrong_storage_and_arguments() {
    for (function, arity) in [
        (new as host_builtins::HostBuiltinFn, 1),
        (len, 1),
        (deque_push, 3),
        (deque_pop, 2),
        (deque_get, 2),
        (heap_from, 1),
        (heap_push, 2),
        (heap_take, 1),
        (heap_peek, 1),
        (map_put, 3),
        (map_get, 2),
        (map_has, 2),
        (map_take, 2),
        (map_edge, 2),
        (map_bound, 3),
        (map_range, 3),
        (map_drain, 1),
    ] {
        assert!(function(vec![], None)
            .unwrap_err()
            .message
            .contains("arguments"));
        assert!(function(vec![Value::Unit; arity], None).is_err());
    }
    assert!(supported(vec![], None).is_err());
    assert!(new(vec![number(4)], None).is_err());
    let map = call(new, vec![number(3)]);
    assert!(map_bound(vec![map.clone(), number(0), number(7)], None).is_err());
    assert!(deque_get(vec![map, number(0)], None).is_err());
}
