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

#[test]
fn native_scalar_stores_round_trip_through_standard_operations() {
    let mut queue = I64Op::PushFront.new_store(false);
    queue.write_i64(I64Op::PushFront, 12, 0).unwrap();
    let saved = Value::CollectionStore(queue.clone());
    assert_eq!(queue.write_i64(I64Op::PopBack, 0, 0).unwrap(), Some(12));
    assert_eq!(
        call(deque_get, vec![saved, number(-1)]).render(),
        "Lookup.Found(12)"
    );

    for maximum in [false, true] {
        let mut heap = I64Op::HeapPush.new_store(maximum);
        heap.write_i64(I64Op::HeapPush, 11, 0).unwrap();
        let state = call(heap_push, vec![Value::CollectionStore(heap), number(4)]);
        let (_, first) = split(call(heap_take, vec![state]));
        assert_eq!(first, number(if maximum { 11 } else { 4 }));
    }

    for (create, unit) in [(I64Op::MapSet, false), (I64Op::SetAdd, true)] {
        let mut tree = create.new_store(false);
        tree.write_i64(create, 3, 7).unwrap();
        tree.write_i64(create, 1, 9).unwrap();
        let state = Value::CollectionStore(tree);
        assert_eq!(call(len, vec![state.clone()]), number(2));
        assert_eq!(
            call(map_has, vec![state.clone(), number(1)]),
            Value::Bool(true)
        );
        let expected = if unit {
            "Lookup.Found((1, None))"
        } else {
            "Lookup.Found((1, 9))"
        };
        assert_eq!(
            call(map_bound, vec![state.clone(), number(2), number(0)]).render(),
            expected
        );
        let (remaining, removed) = split(call(map_take, vec![state, number(3)]));
        assert_eq!(
            removed.render(),
            if unit {
                "Lookup.Found(None)"
            } else {
                "Lookup.Found(7)"
            }
        );
        assert_eq!(call(len, vec![remaining]), number(1));
    }
}

#[test]
fn scalar_layouts_compare_and_iterate_like_the_general_store() {
    for (kind, op) in [
        (0, I64Op::PushBack),
        (1, I64Op::HeapPush),
        (2, I64Op::HeapPush),
        (3, I64Op::MapSet),
    ] {
        let mut general = call(new, vec![number(kind)]);
        let mut packed = op.new_store(kind == 2);
        for n in [5, 1, 8, 3] {
            packed.write_i64(op, n, n + 10).unwrap();
            general = match kind {
                0 => call(deque_push, vec![general, number(n), Value::Bool(false)]),
                1 | 2 => call(heap_push, vec![general, number(n)]),
                _ => call(map_put, vec![general, number(n), number(n + 10)]),
            };
        }
        let state = Value::CollectionStore(packed);
        assert_eq!(general, state);
        if kind == 3 {
            for (op, arguments) in [
                (
                    map_range as host_builtins::HostBuiltinFn,
                    vec![number(2), number(8)],
                ),
                (map_bound, vec![number(4), number(1)]),
                (map_edge, vec![Value::Bool(true)]),
                (map_drain, vec![]),
            ] {
                let with_state = |state: Value| {
                    let mut args = vec![state];
                    args.extend(arguments.clone());
                    call(op, args)
                };
                assert_eq!(with_state(general.clone()), with_state(state.clone()));
            }
        }
    }
}

#[test]
fn deque_host_and_word_operations_share_order_and_empty_pop_errors() {
    for empty in [
        call(new, vec![number(0)]),
        Value::CollectionStore(I64Op::PushBack.new_store(false)),
    ] {
        let queue = call(deque_push, vec![empty, number(5), Value::Bool(false)]);
        let queue = call(deque_push, vec![queue, number(3), Value::Bool(true)]);
        let mut copy = store(queue.clone()).unwrap();
        copy.write_i64(I64Op::PushBack, 8, 0).unwrap();
        assert_eq!(copy.write_i64(I64Op::PopFront, 0, 0).unwrap(), Some(3));
        assert_eq!(copy.write_i64(I64Op::PopBack, 0, 0).unwrap(), Some(8));
        assert_eq!(copy.write_i64(I64Op::PopBack, 0, 0).unwrap(), Some(5));
        let error = copy.write_i64(I64Op::PopFront, 0, 0).unwrap_err();
        assert_eq!(error.code, "AU4003");

        let (queue, last) = split(call(deque_pop, vec![queue, Value::Bool(false)]));
        let (queue, first) = split(call(deque_pop, vec![queue, Value::Bool(true)]));
        assert_eq!((first, last), (number(3), number(5)));
        let error = deque_pop(vec![queue, Value::Bool(false)], None).unwrap_err();
        assert_eq!(error.code, "AU4003");
    }
}

#[test]
fn heap_host_peek_and_word_pop_agree_across_storage_layouts() {
    for (kind, descending, first, second) in [(1, false, 2, 7), (2, true, 7, 2)] {
        for empty in [
            call(new, vec![number(kind)]),
            Value::CollectionStore(I64Op::HeapPush.new_store(descending)),
        ] {
            let heap = call(heap_push, vec![empty, number(7)]);
            let mut mixed = store(heap).unwrap();
            mixed.write_i64(I64Op::HeapPush, 2, 0).unwrap();
            let snapshot = Value::CollectionStore(mixed.clone());
            assert_eq!(
                call(heap_peek, vec![snapshot.clone()]),
                lookup(Some(number(first)))
            );
            assert_eq!(mixed.write_i64(I64Op::HeapPop, 0, 0).unwrap(), Some(first));
            assert_eq!(mixed.write_i64(I64Op::HeapPop, 0, 0).unwrap(), Some(second));
            let error = mixed.write_i64(I64Op::HeapPop, 0, 0).unwrap_err();
            assert_eq!(error.code, "AU4003");
            assert_eq!(call(len, vec![snapshot]), number(2));
        }
    }
}

#[test]
fn map_host_and_word_operations_preserve_overwrites_and_snapshots() {
    for empty in [
        call(new, vec![number(3)]),
        Value::CollectionStore(I64Op::MapSet.new_store(false)),
    ] {
        let original = call(map_put, vec![empty, number(3), number(12)]);
        let mut copy = store(original.clone()).unwrap();
        assert_eq!(copy.read_i64(I64Op::MapGet, 3).unwrap(), Some(12));
        copy.write_i64(I64Op::MapSet, 3, 19).unwrap();
        let updated = Value::CollectionStore(copy.clone());
        assert_eq!(
            call(map_get, vec![updated, number(3)]),
            lookup(Some(number(19)))
        );
        assert_eq!(copy.write_i64(I64Op::MapRemove, 3, 0).unwrap(), Some(19));
        assert_eq!(copy.read_i64(I64Op::MapGet, 3).unwrap(), None);
        assert_eq!(
            call(map_get, vec![original, number(3)]),
            lookup(Some(number(12)))
        );
    }
}

#[test]
fn confined_words_detach_once_and_survive_buffer_growth() {
    for (push, pop, descending) in [
        (I64Op::PushBack, I64Op::PopFront, false),
        (I64Op::HeapPush, I64Op::HeapPop, false),
        (I64Op::HeapPush, I64Op::HeapPop, true),
    ] {
        let mut store = push.new_store(descending);
        store.write_i64(push, 0, 0).unwrap();
        let snapshot = store.clone();
        let address = store.local_words().unwrap();
        // SAFETY: store stays live and is only accessed through this
        // address until the final pop. Its earlier snapshot is detached.
        unsafe {
            for value in 1..1024 {
                assert_eq!(Store::local_step(address, push as i64, value), Some(0));
            }
            for value in 0..1024 {
                let expected = if descending { 1023 - value } else { value };
                assert_eq!(Store::local_step(address, pop as i64, 0), Some(expected));
            }
            assert_eq!(Store::local_step(address, pop as i64, 0), None);
        }
        assert_eq!(call(len, vec![Value::CollectionStore(snapshot)]), number(1));
    }
}
