use super::*;
use crate::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use crate::runtime_value::{InstanceValue, TupleValue, UnionValue};
use aura_core::types::Type;
use std::collections::BTreeMap;

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn text(value: &str) -> Value {
    Value::String(value.to_string().into())
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
        union_type: union_type.into(),
        member_index,
        payload: int(3),
    }));
    assert_eq!(union, int(3));
    assert_eq!(hash_value(&union), hash_value(&int(3)));

    let mut fields = BTreeMap::new();
    fields.insert("x".to_string(), int(1));
    let plain = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("Point".to_string()),
        fields: fields.clone().into(),
    });
    fields.insert(DIRECT_RUNTIME_TYPE_FIELD.to_string(), text("Point"));
    let tagged = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("Point".to_string()),
        fields: fields.into(),
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
    assert_eq!(entries.get(0).unwrap().0, int(99));
    assert_eq!(entries.get(99).unwrap().0, int(0));
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
    let removed = entries.remove(&text("k3"));
    assert_eq!(removed, Some((text("k3"), int(3))));
    assert_eq!(entries.remove(&text("k3")), None);
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
        let key = entries.first().unwrap().0.clone();
        entries.remove(&key);
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
    let clone = DictEntries::cloned_from(entries.clone().into_vec(), &entries);
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
    assert_eq!(elements.get(1), Some(&int(6)));
    assert_eq!(elements.shift_remove_index(0), Some(int(0)));
    assert_eq!(elements.shift_remove_index(48), None);
    assert!(elements.contains_key(&int(147)));
    assert_eq!(elements.pop(), Some(int(147)));
    assert!(!elements.contains_key(&int(147)));
    assert!(elements.contains_key(&int(9)));
}

/// Integers of every width, each equal to `value`, in both representations
/// where the value allows.
fn integers_equal_to(value: i128) -> Vec<Value> {
    let mut integers = Vec::new();
    for kind in [
        IntegerKind::Int8,
        IntegerKind::Int16,
        IntegerKind::Int32,
        IntegerKind::Int64,
        IntegerKind::Int128,
        IntegerKind::Uint8,
        IntegerKind::Uint16,
        IntegerKind::Uint32,
        IntegerKind::Uint64,
        IntegerKind::Uint128,
    ] {
        let typed = if value < 0 || kind.is_signed() {
            IntegerValue::from_typed_signed(value, kind)
        } else {
            IntegerValue::from_typed_unsigned(value as u128, kind)
        };
        if let Some(typed) = typed {
            integers.push(Value::Int(typed));
        }
    }
    integers.push(Value::Int(IntegerValue::from_representation(
        IntegerRepresentation::Signed(value),
    )));
    if value >= 0 {
        integers.push(Value::Int(IntegerValue::from_representation(
            IntegerRepresentation::Unsigned(value as u128),
        )));
    }
    integers
}

#[test]
fn equal_integers_of_every_width_hash_the_same() {
    let wide = i128::from(i64::MAX) + 1;
    for value in [
        0,
        1,
        -1,
        7,
        255,
        -128,
        i128::from(i64::MIN),
        i128::from(i64::MAX),
        wide,
        -wide - 1,
        i128::MAX,
    ] {
        let integers = integers_equal_to(value);
        assert!(integers.len() >= 2, "{value} has several widths");
        let hash = hash_value(&integers[0]);
        for integer in &integers {
            assert_eq!(*integer, integers[0], "{value} compares equal");
            assert_eq!(hash_value(integer), hash, "{value} as {integer:?}");
        }
        if let Ok(narrow) = i64::try_from(value) {
            assert_eq!(hash_i64(narrow), hash, "{value} hashes as an i64");
        }
    }
    let top = Value::Int(IntegerValue::from_literal(u128::MAX));
    assert_eq!(hash_value(&top), hash_value(&top.clone()));
    assert_ne!(hash_value(&int(1)), hash_value(&Value::Bool(true)));
}

#[test]
fn int64_probes_agree_with_value_probes_across_widths() {
    for count in [3_i64, 40] {
        let mut elements = SetElements::new();
        for index in 0..count {
            let value = i128::from(index * 7 - 70);
            let widths = integers_equal_to(value);
            elements.insert(widths[index as usize % widths.len()].clone());
        }
        elements.insert(text("7"));
        let mut entries = DictEntries::new();
        for (position, element) in elements.iter().enumerate() {
            entries.insert(element.clone(), int(position as i64));
        }
        for probe in [
            i64::MIN,
            -70,
            -7,
            -1,
            0,
            7,
            8,
            7 * 39 - 70,
            7 * 40 - 70,
            i64::MAX,
        ] {
            assert_eq!(elements.position_i64(probe), elements.position(&int(probe)));
            assert_eq!(entries.position_i64(probe), entries.position(&int(probe)));
            for width in integers_equal_to(i128::from(probe)) {
                assert_eq!(elements.position(&width), elements.position(&int(probe)));
            }
        }
        let mut copy = elements.clone();
        assert!(!copy.insert_i64(-70));
        assert!(!copy.insert_i64(7 * (count - 1) - 70));
        assert!(copy.insert_i64(1));
        assert!(!copy.insert(int(1)));
        assert!(!copy.insert(integers_equal_to(1)[0].clone()));
        assert_eq!(copy.len(), elements.len() + 1);
        assert_eq!(copy.last(), Some(&int(1)));
        assert_eq!(copy.position_i64(1), Some(elements.len()));
        assert_eq!(elements.position_i64(1), None);
    }
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
        assert!(std::ptr::eq(
            copy.first().unwrap(),
            original.first().unwrap()
        ));

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

        let mut holed = original.clone();
        holed.remove(&int(1));
        assert_eq!(holed.len(), size - 1);
        assert_eq!(original.len(), size);
        assert_eq!(original.value_of(&int(1)), Some(&text("v")));

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

#[test]
fn string_keys_hash_the_same_bare_and_inside_a_union() {
    let text = Value::String("ready".to_string().into());
    let union_type = Type::normalize_union(
        vec![Type::named("str"), Type::named("int64")],
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
        .position(|member| *member == Type::named("str"))
        .expect("str member");
    let union = Value::Union(Box::new(UnionValue {
        union_type: union_type.into(),
        member_index,
        payload: text.clone(),
    }));
    assert_eq!(hash_value(&union), hash_value(&text));
    assert_ne!(
        hash_value(&text),
        hash_value(&Value::String("readz".to_string().into()))
    );
}

#[test]
fn str_lookups_agree_with_value_lookups_in_small_and_large_maps() {
    for count in [3_usize, 40] {
        let mut entries = DictEntries::new();
        for index in 0..count {
            entries.insert(
                Value::String(format!("key-{index}").into()),
                int(index as i64),
            );
        }
        entries.insert(int(1), int(-1));
        for probe in ["key-0", "key-2", "key-39", "key-40", "", "1"] {
            let needle = Value::String(probe.to_string().into());
            assert_eq!(hash_str(probe), hash_value(&needle));
            assert_eq!(entries.position_str(probe), entries.position(&needle));
            assert_eq!(entries.value_of_str(probe), entries.value_of(&needle));
        }
    }
}

fn key(index: i64) -> Value {
    text(&format!("k{index}"))
}

#[test]
fn map_removal_leaves_holes_that_reads_skip() {
    let mut entries = DictEntries::new();
    for index in 0..100 {
        entries.insert(key(index), int(index));
    }
    for index in 10..21 {
        assert_eq!(entries.remove(&key(index)), Some((key(index), int(index))));
    }
    let live: Vec<i64> = (0..100).filter(|index| !(10..21).contains(index)).collect();
    assert_eq!(entries.len(), live.len());
    assert_eq!(
        keys(&entries),
        live.iter().map(|index| key(*index)).collect::<Vec<_>>()
    );
    let mut reversed: Vec<Value> = entries.iter().rev().map(|(key, _)| key.clone()).collect();
    reversed.reverse();
    assert_eq!(reversed, keys(&entries));
    // Positions name slots, which removals leave in place.
    assert_eq!(entries.position(&key(21)), Some(21));
    assert_eq!(entries.position_str("k21"), Some(21));
    assert_eq!(entries.at_slot(21), &(key(21), int(21)));
    assert_eq!(entries.slot_where(|(_, value)| *value == int(30)), Some(30));
    assert_eq!(entries.slot_where(|(_, value)| *value == int(15)), None);
    // Indexes count entries.
    assert_eq!(entries.get(10), Some(&(key(21), int(21))));
    assert_eq!(entries.get(live.len()), None);
    assert_eq!(entries.first(), Some(&(key(0), int(0))));
    assert_eq!(entries.last(), Some(&(key(99), int(99))));
    let mut iter = entries.iter();
    assert_eq!(iter.len(), live.len());
    iter.next();
    let copy = iter.clone();
    assert_eq!(copy.size_hint(), (live.len() - 1, Some(live.len() - 1)));
    assert!(entries.capacity() >= entries.len());
    *entries.value_at_mut(30) = int(-30);
    assert_eq!(entries.value_of(&key(30)), Some(&int(-30)));

    // A new key, and a key put back after its removal, go to the end.
    entries.insert(key(15), int(15));
    entries.insert(key(100), int(100));
    assert_eq!(entries.last(), Some(&(key(100), int(100))));
    assert_eq!(entries.get(entries.len() - 2), Some(&(key(15), int(15))));
    assert!(
        entries == {
            let mut expected: Vec<(Value, Value)> = live
                .iter()
                .map(|index| (key(*index), int(if *index == 30 { -30 } else { *index })))
                .collect();
            expected.push((key(15), int(15)));
            expected.push((key(100), int(100)));
            expected
        }
    );
    assert_eq!(entries.clone().into_vec().len(), entries.len());

    // A copy built in order reuses the index, with its slots packed.
    let copy = DictEntries::cloned_from(entries.clone().into_vec(), &entries);
    for index in live.iter().chain(&[15, 100]) {
        assert!(copy.contains_key(&key(*index)));
    }
    assert_eq!(copy.position(&key(21)), Some(10));
    entries.try_reserve(50).expect("reserve");
    assert!(entries.capacity() >= entries.len() + 50);
}

#[test]
fn removal_packs_slots_once_holes_pass_half() {
    let mut entries = DictEntries::new();
    for index in 0..40 {
        entries.insert(key(index), int(index));
    }
    // Removing the last entry leaves no hole behind it.
    entries.remove(&key(39));
    entries.remove(&key(37));
    entries.remove(&key(38));
    assert_eq!(entries.len(), 37);
    assert_eq!(entries.last(), Some(&(key(36), int(36))));
    for index in 0..18 {
        entries.remove(&key(index));
    }
    // 18 holes in 37 slots stay; the next one packs.
    assert_eq!(entries.position(&key(18)), Some(18));
    entries.remove(&key(18));
    assert_eq!(entries.position(&key(19)), Some(0));
    assert_eq!(entries.position(&key(36)), Some(17));
    assert_eq!(keys(&entries), (19..37).map(key).collect::<Vec<_>>());

    // A shifting removal closes every hole at once.
    entries.remove(&key(20));
    entries.shift_remove(&key(22));
    assert_eq!(entries.position(&key(23)), Some(2));
    entries.remove(&key(24));
    assert_eq!(entries.shift_remove_index(1), Some((key(21), int(21))));
    assert_eq!(entries.position(&key(23)), Some(1));
    assert_eq!(entries.shift_remove_index(entries.len()), None);
    entries.remove(&key(26));
    assert_eq!(entries.pop(), Some((key(36), int(36))));

    // Falling to the threshold scans again.
    while entries.len() > INDEX_THRESHOLD {
        let first = entries.first().unwrap().0.clone();
        entries.remove(&first);
    }
    assert_eq!(keys(&entries), (28..36).map(key).collect::<Vec<_>>());
    assert_eq!(entries.position(&key(28)), Some(0));
}

#[test]
fn removal_and_insertion_in_turn_match_an_ordered_model() {
    // An LRU cache's pattern: remove an older key, insert a new one.
    let mut entries = DictEntries::new();
    let mut model: Vec<i64> = Vec::new();
    let mut state: i64 = 7;
    for index in 0..64 {
        entries.insert(int(index), int(index));
        model.push(index);
    }
    for next in 64..2000 {
        state = state * 48271 % 2147483647;
        let victim = model.remove((state as usize) % model.len());
        assert_eq!(
            entries.remove(&int(victim)),
            Some((int(victim), int(victim)))
        );
        entries.insert(int(next), int(next));
        model.push(next);
        assert_eq!(entries.len(), model.len());
        if next % 97 == 0 {
            let order: Vec<Value> = entries.iter().map(|(key, _)| key.clone()).collect();
            assert_eq!(
                order,
                model.iter().map(|index| int(*index)).collect::<Vec<_>>()
            );
            for (position, index) in model.iter().enumerate() {
                assert_eq!(entries.get(position), Some(&(int(*index), int(*index))));
            }
        }
    }
    for index in &model {
        assert_eq!(entries.value_of_i64(*index), Some(&int(*index)));
    }
}

#[test]
fn clearing_keeps_room_unless_shared() {
    let mut entries: DictEntries = (0..20).map(|index| (int(index), int(index))).collect();
    entries.remove(&int(3));
    let capacity = entries.capacity();
    let shared = entries.clone();
    entries.clear();
    assert!(entries.is_empty());
    assert_eq!(shared.len(), 19);
    let mut unshared = shared;
    unshared.clear();
    assert!(unshared.capacity() >= capacity.min(19));
    assert!(unshared.is_empty());
}

#[test]
#[allow(clippy::manual_next_back)]
fn small_and_large_entries_answer_the_same_reads() {
    for size in [4_i64, 40] {
        let entries: DictEntries = (0..size).map(|index| (int(index), int(-index))).collect();
        let iter = entries.iter();
        assert_eq!(iter.clone().count(), size as usize);
        assert_eq!(iter.rev().next(), Some(&(int(size - 1), int(1 - size))));
        assert_eq!(entries.last(), Some(&(int(size - 1), int(1 - size))));
        assert_eq!(entries.slot_where(|(_, value)| *value == int(-2)), Some(2));
        assert_eq!(entries.slot_where(|(_, value)| *value == int(1)), None);
        assert_eq!(entries.at_slot(3), &(int(3), int(-3)));
        assert_eq!(entries.get(3), Some(&(int(3), int(-3))));
        assert_eq!(entries.get(size as usize), None);
        let copy = entries.clone();
        assert_eq!(copy.as_ptr(), entries.as_ptr());
        assert!(entries == copy.into_vec());
    }
}

#[test]
fn small_and_large_dicts_match_str_and_integer_keys_by_value() {
    let typed_seven = Value::Int(
        IntegerValue::from_typed_signed(7, IntegerKind::Int32).expect("7 fits in int32"),
    );
    let long_key = "a key longer than sixteen bytes";
    for size in [3, INDEX_THRESHOLD + 3] {
        let mut entries = DictEntries::new();
        for index in 0..size {
            entries.insert(text(&format!("k{index}")), int(index as i64));
        }
        entries.insert(int(7), text("seven"));
        entries.insert(text(long_key), text("long"));
        entries.insert(text(""), text("empty"));

        // An equal key keeps its first place and takes the later value.
        assert_eq!(entries.insert(text("k1"), int(10)), Some(int(1)));
        assert_eq!(
            entries.insert(typed_seven.clone(), text("typed")),
            Some(text("seven"))
        );
        assert_eq!(entries.len(), size + 3);
        assert_eq!(entries.get(1), Some(&(text("k1"), int(10))));
        assert_eq!(entries.value_of(&int(7)), Some(&text("typed")));
        assert_eq!(entries.value_of(&text(long_key)), Some(&text("long")));
        assert_eq!(entries.value_of(&text("")), Some(&text("empty")));
        // Keys of one length that differ only inside, and keys of another
        // kind, do not match.
        assert_eq!(
            entries.value_of(&text("a key longer than sixteen bytez")),
            None
        );
        assert_eq!(entries.value_of(&text("k")), None);
        assert_eq!(entries.value_of(&Value::Float(7.0)), None);
        assert_eq!(
            entries.remove(&text("k0")).map(|(_, value)| value),
            Some(int(0))
        );
        assert_eq!(entries.value_of(&text("k0")), None);
    }

    let mut set = SetElements::new();
    assert!(set.insert(text("a")));
    assert!(!set.insert(text("a")));
    assert!(set.insert(int(7)));
    assert!(!set.insert(typed_seven));
}

/// SipHash-2-4 with key bytes 0..16 over messages of bytes 0..n, from the
/// reference implementation's test vectors.
#[test]
fn sip_hasher_matches_the_reference_vectors() {
    use super::SipHasher;
    use std::hash::Hasher;
    let k0 = u64::from_le_bytes([0, 1, 2, 3, 4, 5, 6, 7]);
    let k1 = u64::from_le_bytes([8, 9, 10, 11, 12, 13, 14, 15]);
    let message: Vec<u8> = (0..15).collect();
    for (len, expected) in [
        (0, 0x726f_db47_dd0e_0e31u64),
        (1, 0x74f8_39c5_93dc_67fd),
        (15, 0xa129_ca61_49be_45e5),
    ] {
        let mut hasher = SipHasher::<2, 4>::with_keys(k0, k1);
        hasher.write(&message[..len]);
        assert_eq!(hasher.finish(), expected, "message of {len} bytes");
    }
}

/// `insert_i64` stores as `insert` does under the same integer key, in a
/// scanned dict and an indexed one, and keeps the first key and its slot.
#[test]
fn int64_inserts_agree_with_value_inserts() {
    for size in [3i64, 40] {
        let mut by_word = DictEntries::new();
        let mut by_value = DictEntries::new();
        for round in 0..2 {
            for key in 0..size {
                let stored = Value::Int(IntegerValue::from_i64(key + round));
                assert_eq!(
                    by_word.insert_i64(key * 11, stored.clone()),
                    by_value.insert(Value::Int(IntegerValue::from_i64(key * 11)), stored),
                    "key {key} in round {round}"
                );
            }
        }
        assert_eq!(by_word.len(), size as usize);
        assert!(by_word.iter().eq(by_value.iter()));
        for key in 0..size {
            assert_eq!(by_word.position_i64(key * 11), Some(key as usize));
            assert_eq!(by_word.position_i64(key * 11 + 1), None);
        }
    }
}

/// `remove_i64` removes what `remove` does under the same integer key.
#[test]
fn int64_removes_agree_with_value_removes() {
    for size in [4i64, 40] {
        let mut by_word = DictEntries::new();
        let mut by_value = DictEntries::new();
        for key in 0..size {
            by_word.insert(int(key), int(key * 2));
            by_value.insert(int(key), int(key * 2));
        }
        for key in [1, 1, size - 1, 0, size + 5, -3] {
            assert_eq!(by_word.remove_i64(key), by_value.remove(&int(key)), "{key}");
            assert_eq!(by_word.len(), by_value.len());
            assert!(by_word.iter().eq(by_value.iter()));
        }
    }
}

/// The one-shot hashes of a `str` or integer key are the hashes a stream
/// gives the same bytes, at every length around the word boundaries.
#[test]
fn one_shot_hashes_match_the_stream() {
    use super::SipHasher;
    use std::hash::Hasher;
    let k0 = u64::from_le_bytes([0, 1, 2, 3, 4, 5, 6, 7]);
    let k1 = u64::from_le_bytes([8, 9, 10, 11, 12, 13, 14, 15]);
    let message: Vec<u8> = (0..80u8).map(|byte| byte.wrapping_mul(37) ^ 0x5a).collect();
    for len in 0..=message.len() {
        let mut stream = SipHasher::<1, 3>::with_keys(k0, k1);
        stream.write(&message[..len]);
        assert_eq!(
            SipHasher::<1, 3>::hash_bytes(k0, k1, &message[..len]),
            stream.finish(),
            "message of {len} bytes"
        );
    }
    let reference: Vec<u8> = (0..15).collect();
    for (len, expected) in [
        (0, 0x726f_db47_dd0e_0e31u64),
        (1, 0x74f8_39c5_93dc_67fd),
        (15, 0xa129_ca61_49be_45e5),
    ] {
        assert_eq!(
            SipHasher::<2, 4>::hash_bytes(k0, k1, &reference[..len]),
            expected,
            "reference message of {len} bytes"
        );
    }
    for word in [0, 1, u64::MAX, 0x0123_4567_89ab_cdef] {
        let mut stream = SipHasher::<1, 3>::with_keys(k0, k1);
        stream.write_u64(word);
        assert_eq!(SipHasher::<1, 3>::hash_word(k0, k1, word), stream.finish());
        assert_eq!(
            SipHasher::<1, 3>::hash_bytes(k0, k1, &word.to_le_bytes()),
            stream.finish()
        );
    }
}

#[test]
fn keyed_hasher_hashes_split_writes_and_words_as_one_stream() {
    use super::KeyedHasher;
    use std::hash::Hasher;
    let bytes: Vec<u8> = (0..41u8).map(|byte| byte.wrapping_mul(7)).collect();
    let mut whole = KeyedHasher::with_keys(3, 9);
    whole.write(&bytes);
    for split in [0, 1, 7, 8, 9, 16, 23, 40, 41] {
        for second in split..=bytes.len() {
            let mut parts = KeyedHasher::with_keys(3, 9);
            parts.write(&bytes[..split]);
            parts.write(&bytes[split..second]);
            parts.write(&bytes[second..]);
            assert_eq!(
                parts.finish(),
                whole.finish(),
                "split at {split} and {second}"
            );
        }
    }
    for prefix in 0..9 {
        let word = 0x0123_4567_89ab_cdefu64;
        let mut by_word = KeyedHasher::with_keys(5, 11);
        by_word.write(&bytes[..prefix]);
        by_word.write_u64(word);
        by_word.write_i64(-2);
        let mut by_bytes = KeyedHasher::with_keys(5, 11);
        by_bytes.write(&bytes[..prefix]);
        by_bytes.write(&word.to_le_bytes());
        by_bytes.write(&(-2i64).to_le_bytes());
        assert_eq!(by_word.finish(), by_bytes.finish(), "after {prefix} bytes");
    }
    assert_ne!(
        KeyedHasher::with_keys(1, 2).finish(),
        KeyedHasher::with_keys(2, 1).finish()
    );
}

#[test]
fn lookups_in_entry_order_and_after_removals_find_every_key() {
    let mut entries = DictEntries::new();
    for key in 0..100 {
        entries.insert(int(key), int(key * 10));
    }
    for key in (1..100).step_by(3) {
        entries.remove(&int(key));
    }
    let live: Vec<i64> = (0..100).filter(|key| key % 3 != 1).collect();
    // In order, the hint finds most keys; out of order and missing keys
    // fall back to the index.
    for pass in [live.clone(), live.iter().rev().copied().collect()] {
        for key in pass {
            assert_eq!(entries.value_of_i64(key), Some(&int(key * 10)), "key {key}");
            assert_eq!(entries.value_of(&int(key)), Some(&int(key * 10)));
        }
    }
    for key in (1..100).step_by(3) {
        assert_eq!(entries.value_of_i64(key), None, "removed key {key}");
    }
    assert_eq!(entries.value_of_i64(1000), None);
    let mut texts = DictEntries::new();
    for key in 0..20 {
        texts.insert(text(&format!("k{key}")), int(key));
    }
    for key in [0, 1, 2, 19, 5, 6, 0] {
        assert_eq!(texts.value_of_str(&format!("k{key}")), Some(&int(key)));
    }
    assert_eq!(texts.value_of_str("k20"), None);
}

#[test]
fn slots_walk_the_entries_in_order_past_removed_ones() {
    // Walks the entries by slot, as a keys loop over an unchanged dict does.
    fn walk(entries: &DictEntries) -> Vec<Value> {
        let mut found = Vec::new();
        let mut slot = 0;
        while let Some(next) = entries.next_slot(slot) {
            let (key, _) = entries.slot(next).expect("a live slot holds an entry");
            found.push(key.clone());
            slot = next + 1;
        }
        found
    }
    let mut small = DictEntries::new();
    for key in 0..4 {
        small.insert(int(key), int(key * 10));
    }
    small.remove(&int(1));
    assert_eq!(walk(&small), vec![int(0), int(2), int(3)]);
    assert_eq!(small.slot(3), None);

    let mut large = DictEntries::new();
    for key in 0..100 {
        large.insert(int(key), int(key * 10));
    }
    for key in (1..100).step_by(3) {
        large.remove(&int(key));
    }
    let live: Vec<Value> = (0..100).filter(|key| key % 3 != 1).map(int).collect();
    assert_eq!(walk(&large), live);
    assert_eq!(large.slot(1), None);
    assert_eq!(large.next_slot(usize::MAX), None);
    assert_eq!(DictEntries::new().next_slot(0), None);
}

fn set_of(values: &[i64]) -> SetElements {
    let mut set = SetElements::new();
    for value in values {
        set.insert(int(*value));
    }
    set
}

fn algebra_elements(method: &str, left: &[i64], right: &[i64]) -> Vec<Value> {
    match set_algebra(method, &set_of(left), &set_of(right)).expect("a set-algebra method") {
        SetAlgebra::Elements(elements) => elements.iter().cloned().collect(),
        SetAlgebra::Holds(_) => panic!("`{method}` builds a set"),
    }
}

fn algebra_holds(method: &str, left: &[i64], right: &[i64]) -> bool {
    match set_algebra(method, &set_of(left), &set_of(right)).expect("a set-algebra method") {
        SetAlgebra::Holds(holds) => holds,
        SetAlgebra::Elements(_) => panic!("`{method}` answers a question"),
    }
}

#[test]
fn union_with_a_subset_shares_the_receivers_storage_until_a_write() {
    let left = set_of(&(0..64).collect::<Vec<_>>());
    let right = set_of(&[9, 2, 31]);
    let Some(SetAlgebra::Elements(mut union)) = set_algebra("union", &left, &right) else {
        panic!("union produces a set");
    };
    assert!(std::ptr::eq(
        left.iter().next().unwrap(),
        union.iter().next().unwrap()
    ));
    union.insert(int(64));
    assert!(!left.contains_key(&int(64)));
    assert_eq!(union.len(), 65);
}

#[test]
fn indexed_union_appends_in_insertion_order_across_removed_slots() {
    let mut left = set_of(&(0..40).collect::<Vec<_>>());
    let mut right = set_of(&(25..65).rev().collect::<Vec<_>>());
    left.remove(&int(3));
    right.remove(&int(43));
    right.insert(int(3));
    let Some(SetAlgebra::Elements(union)) = set_algebra("union", &left, &right) else {
        panic!("union should return a set");
    };
    let expected = (0..40)
        .filter(|n| *n != 3)
        .chain((40..65).rev().filter(|n| *n != 43))
        .chain([3])
        .map(int)
        .collect::<Vec<_>>();
    assert_eq!(union.iter().cloned().collect::<Vec<_>>(), expected);
    assert_eq!(union.len(), expected.len());
    assert!(expected.iter().all(|key| union.contains_key(key)));
    assert!(!left.contains_key(&int(64)));
    assert!(!right.contains_key(&int(43)));
}

#[test]
fn set_algebra_keeps_the_receiver_order_then_the_other_sets() {
    let ints = |values: &[i64]| values.iter().map(|value| int(*value)).collect::<Vec<_>>();
    assert_eq!(
        algebra_elements("union", &[4, 1, 2], &[2, 9, 1, 7]),
        ints(&[4, 1, 2, 9, 7])
    );
    assert_eq!(
        algebra_elements("intersection", &[4, 1, 2], &[2, 9, 4]),
        ints(&[4, 2])
    );
    assert_eq!(
        algebra_elements("difference", &[4, 1, 2], &[2, 9, 4]),
        ints(&[1])
    );
    assert_eq!(
        algebra_elements("symmetric_difference", &[4, 1, 2], &[2, 9, 4, 7]),
        ints(&[1, 9, 7])
    );
    for method in [
        "union",
        "intersection",
        "difference",
        "symmetric_difference",
    ] {
        assert!(algebra_elements(method, &[], &[]).is_empty());
    }
    assert_eq!(algebra_elements("union", &[], &[3]), ints(&[3]));
    assert!(algebra_elements("difference", &[3], &[3]).is_empty());
    assert!(set_algebra("shuffle", &set_of(&[1]), &set_of(&[1])).is_none());
    assert_eq!(SET_ALGEBRA_METHODS.len(), 7);
    assert_eq!(SET_ALGEBRA_FIRST_OPCODE, 8);
}

#[test]
fn set_relations_hold_for_empty_equal_and_nested_sets() {
    assert!(algebra_holds("is_subset", &[], &[]));
    assert!(algebra_holds("is_subset", &[], &[1]));
    assert!(algebra_holds("is_subset", &[1, 2], &[2, 1]));
    assert!(algebra_holds("is_subset", &[2], &[1, 2, 3]));
    assert!(!algebra_holds("is_subset", &[1, 2, 3], &[1, 2]));
    assert!(!algebra_holds("is_subset", &[1, 4], &[1, 2, 3]));
    assert!(algebra_holds("is_superset", &[1, 2, 3], &[3, 1]));
    assert!(algebra_holds("is_superset", &[1], &[]));
    assert!(!algebra_holds("is_superset", &[1], &[1, 2]));
    assert!(algebra_holds("is_disjoint", &[], &[]));
    assert!(algebra_holds("is_disjoint", &[1, 2], &[3]));
    assert!(algebra_holds("is_disjoint", &[3], &[1, 2]));
    assert!(!algebra_holds("is_disjoint", &[1, 2, 3], &[3]));
    assert!(!algebra_holds("is_disjoint", &[3], &[1, 2, 3]));
}

#[test]
fn shared_union_reads_and_writes_preserve_order_and_source_snapshots() {
    let mut left: SetElements = (0..128).map(|i| int((i * 73) % 128)).collect();
    let mut right: SetElements = (0..128).map(|i| int(64 + (i * 53) % 128)).collect();
    for key in [4, 8, 15, 67, 118] {
        left.remove(&int(key));
    }
    for key in [70, 101, 137, 150] {
        right.remove(&int(key));
    }
    let mut expected: Vec<_> = left.iter().cloned().collect();
    for value in &right {
        if !expected.contains(value) {
            expected.push(value.clone());
        }
    }

    let mut union = left.union(&right);
    let snapshot = union.clone();
    assert_eq!(union.len(), expected.len());
    assert_eq!(
        union.iter().collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
    assert!(union.iter().rev().eq(expected.iter().rev()));
    assert_eq!(union.first(), expected.first());
    assert_eq!(union.last(), expected.last());
    let mut next = union.next_slot(0);
    for (i, value) in expected.iter().enumerate() {
        let slot = next.unwrap();
        assert_eq!(union.slot(slot), Some(value));
        assert_eq!(union.get(i), Some(value));
        assert_eq!(union.slot_where(|key| key == value), Some(slot));
        assert_eq!(union.find(value), Some(value));
        assert_eq!(union.at_slot(union.position(value).unwrap()), value);
        next = union.next_slot(slot + 1);
    }
    assert_eq!(next, None);
    assert_eq!(union.get(expected.len()), None);
    assert!(union.slot(usize::MAX).is_none());
    assert_eq!(union.next_slot(usize::MAX), None);

    assert!(
        matches!(&snapshot.storage, Storage::Union(backing) if backing.realized.get().is_none())
    );
    left.remove(&int(0));
    right.remove(&int(190));
    assert_eq!(union.remove(&int(191)), Some(int(191)));
    expected.retain(|v| v != &int(191));
    assert!(union.insert(int(-7)));
    expected.push(int(-7));
    assert!(union.iter().eq(&expected));
    assert!(snapshot.contains_key(&int(0)));
    assert!(snapshot.contains_key(&int(190)));
    assert!(snapshot.contains_key(&int(191)));
    assert!(!snapshot.contains_key(&int(-7)));

    let mut second = snapshot.clone();
    let shifted = snapshot.get(120).unwrap().clone();
    assert_eq!(second.shift_remove_index(120), Some(shifted.clone()));
    assert!(!second.contains_key(&shifted));
    assert!(snapshot.contains_key(&shifted));
    assert!(second.union(&union).contains_key(&int(-7)));
    let dense = snapshot.clone().into_vec();
    assert!(snapshot.iter().eq(&dense));
    let copy = SetElements::cloned_from(dense, &snapshot);
    assert!(snapshot.iter().eq(copy.iter()));
    assert_eq!(copy.find(&int(191)), Some(&int(191)));

    let mut cleared = snapshot.clone();
    assert!(cleared.is_shared());
    assert_eq!(cleared.capacity(), cleared.len());
    assert_eq!(cleared.as_ptr(), snapshot.as_ptr());
    cleared.clear();
    assert!(cleared.is_empty());
    assert!(!snapshot.is_empty());

    let mut reserved = snapshot.clone();
    reserved.try_reserve(256).unwrap();
    assert!(reserved.capacity() >= reserved.len() + 256);
    assert!(reserved.iter().eq(snapshot.iter()));
}

#[test]
fn union_drops_right_snapshots_with_excluded_owned_payloads() {
    let record = |n: i64| {
        Value::Instance(InstanceValue {
            class_name: "Record".to_string().into(),
            fields: BTreeMap::from([("key".to_string(), int(n))]).into(),
        })
    };
    // Equal records have distinct owned payloads. The result keeps the
    // representative on the left and may release the right-hand duplicate
    // as soon as the input set drops.
    let left: SetElements = (0..16).map(record).collect();
    let right: SetElements = (8..24).map(record).collect();
    let union = left.union(&right);
    assert!(matches!(union.storage, Storage::Large(_)));
    drop(right);
    assert!(union
        .iter()
        .eq((0..24).map(record).collect::<Vec<_>>().iter()));
}
