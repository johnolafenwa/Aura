use super::*;
use crate::integer::{IntegerKind, IntegerValue};
use crate::runtime_value::{InstanceValue, TupleValue, UnionValue};
use aura_core::types::Type;
use std::collections::BTreeMap;

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

fn keys(entries: &DictEntries) -> Vec<Value> {
    entries.iter().map(|(key, _)| key.clone()).collect()
}

#[test]
fn equal_values_hash_the_same() {
    let typed = Value::Int(
        IntegerValue::from_typed_signed(7, IntegerKind::Int32).expect("7 fits in int32"),
    );
    assert_eq!(typed, int(7));
    assert_eq!(hash_value(&typed), hash_value(&int(7)));

    assert_eq!(Value::Float(0.0), Value::Float(-0.0));
    assert_eq!(
        hash_value(&Value::Float(0.0)),
        hash_value(&Value::Float(-0.0))
    );

    let union_type = Type::normalize_union(
        vec![Type::named("int64"), Type::Unit],
        "main",
        &BTreeMap::new(),
    )
    .expect("normalized union");
    let Type::Union(members) = &union_type else {
        panic!("expected a union type");
    };
    let member_index = members
        .members
        .iter()
        .position(|member| *member == Type::named("int64"))
        .expect("int64 member");
    let union = Value::Union(Box::new(UnionValue {
        union_type,
        member_index,
        payload: int(3),
    }));
    assert_eq!(union, int(3));
    assert_eq!(hash_value(&union), hash_value(&int(3)));

    let mut fields = BTreeMap::new();
    fields.insert("x".to_string(), int(1));
    let plain = Value::Instance(InstanceValue {
        class_name: "Point".to_string(),
        fields: fields.clone(),
    });
    fields.insert(DIRECT_RUNTIME_TYPE_FIELD.to_string(), text("Point"));
    let tagged = Value::Instance(InstanceValue {
        class_name: "Point".to_string(),
        fields,
    });
    assert_eq!(plain, tagged);
    assert_eq!(hash_value(&plain), hash_value(&tagged));
}

#[test]
fn different_strings_usually_hash_differently() {
    assert_ne!(hash_value(&text("a")), hash_value(&text("b")));
    assert_ne!(hash_value(&int(1)), hash_value(&int(2)));
}

#[test]
fn deep_keys_hash_without_recursion() {
    let mut value = int(0);
    for _ in 0..100_000 {
        value = Value::Tuple(TupleValue {
            element_types: vec![Type::named("int64")],
            elements: vec![value],
        });
    }
    let hash = hash_value(&value);
    assert_eq!(hash, hash_value(&value));
    // Drop the deep value without recursing through `Drop`.
    let mut pending = vec![value];
    while let Some(next) = pending.pop() {
        if let Value::Tuple(tuple) = next {
            pending.extend(tuple.elements);
        }
    }
}

#[test]
fn map_keeps_insertion_order_across_the_index_threshold() {
    let mut entries = DictEntries::new();
    for index in (0..100).rev() {
        assert_eq!(entries.insert(int(index), int(index * 10)), None);
    }
    assert_eq!(entries.len(), 100);
    assert_eq!(entries[0].0, int(99));
    assert_eq!(entries[99].0, int(0));
    for index in 0..100 {
        assert_eq!(entries.value_of(&int(index)), Some(&int(index * 10)));
    }
    assert_eq!(entries.value_of(&int(100)), None);

    // Replacing a value keeps the slot and the original key.
    assert_eq!(entries.insert(int(50), int(-1)), Some(int(500)));
    assert_eq!(entries.position(&int(50)), Some(49));
    assert_eq!(entries.len(), 100);
}

#[test]
fn map_removal_keeps_the_order_of_the_rest() {
    let mut entries = DictEntries::new();
    for index in 0..20 {
        entries.insert(text(&format!("k{index}")), int(index));
    }
    let removed = entries.shift_remove(&text("k3"));
    assert_eq!(removed, Some((text("k3"), int(3))));
    assert_eq!(entries.shift_remove(&text("k3")), None);
    assert_eq!(entries.len(), 19);
    let expected: Vec<Value> = (0..20)
        .filter(|index| *index != 3)
        .map(|index| text(&format!("k{index}")))
        .collect();
    assert_eq!(keys(&entries), expected);
    for index in (0..20).filter(|index| *index != 3) {
        assert_eq!(
            entries.value_of(&text(&format!("k{index}"))),
            Some(&int(index))
        );
    }

    // Shrinking below the threshold falls back to scanning.
    while entries.len() > 2 {
        let key = entries[0].0.clone();
        entries.shift_remove(&key);
    }
    assert_eq!(entries.value_of(&text("k18")), Some(&int(18)));
    assert_eq!(entries.value_of(&text("k19")), Some(&int(19)));
    entries.clear();
    assert!(entries.is_empty());
    assert_eq!(entries.value_of(&text("k19")), None);
}

#[test]
fn wrapped_entries_find_the_first_equal_key() {
    let mut raw: Vec<(Value, Value)> = (0..12).map(|index| (int(index), int(index))).collect();
    raw.push((int(5), int(-5)));
    let entries = DictEntries::from(raw);
    assert_eq!(entries.value_of(&int(5)), Some(&int(5)));
    let clone = DictEntries::cloned_from(entries.to_vec(), &entries);
    assert_eq!(clone.value_of(&int(11)), Some(&int(11)));
}

#[test]
fn set_insert_contains_and_remove() {
    let mut elements = SetElements::new();
    for index in 0..50 {
        assert!(elements.insert(int(index * 3)));
    }
    assert!(!elements.insert(int(0)));
    assert_eq!(elements.len(), 50);
    assert!(elements.contains_key(&int(147)));
    assert!(!elements.contains_key(&int(148)));
    assert_eq!(elements.shift_remove(&int(3)), Some(int(3)));
    assert!(!elements.contains_key(&int(3)));
    assert_eq!(elements[1], int(6));
    assert_eq!(elements.shift_remove_at(0), int(0));
    assert!(elements.contains_key(&int(147)));
    assert_eq!(elements.pop(), Some(int(147)));
    assert!(!elements.contains_key(&int(147)));
    assert!(elements.contains_key(&int(9)));
}

#[test]
fn collected_entries_index_and_clone_with_their_index() {
    let elements: SetElements = (0..30).map(int).collect();
    assert_eq!(elements.position(&int(29)), Some(29));
    let copy = elements.clone();
    assert_eq!(copy.position(&int(12)), Some(12));
    assert!(copy == (0..30).map(int).collect::<Vec<_>>());
    let small: SetElements = (0..3).map(int).collect();
    assert_eq!(small.clone().into_vec(), vec![int(0), int(1), int(2)]);
}

#[test]
fn indexed_entries_stay_no_larger_than_a_vec() {
    assert!(std::mem::size_of::<DictEntries>() <= std::mem::size_of::<Vec<(Value, Value)>>());
    assert!(std::mem::size_of::<SetElements>() <= std::mem::size_of::<Vec<Value>>());
}

#[test]
fn clones_share_storage_until_a_write() {
    for size in [3, INDEX_THRESHOLD * 4] {
        let original: DictEntries = (0..size as i64)
            .map(|index| (int(index), text("v")))
            .collect();
        let mut copy = original.clone();
        assert!(original.is_shared() && copy.is_shared());
        assert_eq!(copy.as_slice().as_ptr(), original.as_slice().as_ptr());

        copy.insert(int(0), text("changed"));
        assert!(!original.is_shared() && !copy.is_shared());
        assert_eq!(original.value_of(&int(0)), Some(&text("v")));
        assert_eq!(copy.value_of(&int(0)), Some(&text("changed")));

        let mut grown = original.clone();
        grown.insert(int(size as i64), text("new"));
        assert_eq!(original.len(), size);
        assert_eq!(grown.len(), size + 1);
        assert!(!original.contains_key(&int(size as i64)));

        let mut removed = original.clone();
        removed.shift_remove(&int(1));
        assert_eq!(removed.position(&int(2)), Some(1));
        assert_eq!(original.position(&int(2)), Some(2));

        let mut popped = original.clone();
        assert!(popped.pop().is_some());
        assert_eq!(original.len(), size);

        let mut written = original.clone();
        *written.value_at_mut(0) = text("w");
        assert_eq!(original.value_of(&int(0)), Some(&text("v")));

        let mut cleared = original.clone();
        cleared.clear();
        assert!(cleared.is_empty());
        assert_eq!(original.len(), size);
        assert_eq!(original.clone().into_vec().len(), size);

        let mut reserved = original.clone();
        reserved.try_reserve(4).expect("reserve");
        assert!(!original.is_shared());
    }
}

#[test]
fn shared_vecs_copy_on_the_first_write() {
    let original: SharedVec<Value> = vec![int(1), int(2)].into();
    let mut copy = original.clone();
    assert!(original.is_shared());
    assert_eq!(copy.as_ptr(), original.as_ptr());
    copy.push(int(3));
    assert!(!original.is_shared());
    assert_eq!(*original, vec![int(1), int(2)]);
    assert_eq!(copy, vec![int(1), int(2), int(3)]);

    let mut doubled = original.clone();
    for value in &mut doubled {
        *value = int(9);
    }
    assert_eq!(original, vec![int(1), int(2)]);
    assert_eq!(doubled.iter().count(), 2);
    assert_eq!((&doubled).into_iter().count(), 2);
    assert_eq!(original.clone().into_iter().count(), 2);
    assert_eq!(original.clone().into_vec(), vec![int(1), int(2)]);
    assert!(SharedVec::<Value>::default().is_empty());
    assert_eq!((0..3).map(int).collect::<SharedVec<_>>().len(), 3);
    let nan: SharedVec<Value> = vec![Value::Float(f64::NAN)].into();
    assert_ne!(nan, nan.clone());
}
