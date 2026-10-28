use std::borrow::Cow;
use std::sync::OnceLock;

use aura_core::layout::{encode_descriptor, ClassDefinition, LayoutEnv};
use aura_core::types::Type;

use super::*;
use crate::integer::IntegerKind;
use crate::native_runtime::plain_layout_for_descriptor;
use crate::runtime_value::InstanceValue;

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn untyped(value: i64) -> Value {
    Value::Int(IntegerValue::from_signed(i128::from(value)))
}

fn int32(value: i32) -> Value {
    typed(value.into(), IntegerKind::Int32)
}

/// An integer of `kind`, built the way the runtime builds one.
fn typed(value: i128, kind: IntegerKind) -> Value {
    Value::Int(if kind.is_signed() {
        IntegerValue::from_typed_signed(value, kind).expect("the value fits its kind")
    } else {
        IntegerValue::from_typed_unsigned(value as u128, kind).expect("the value fits its kind")
    })
}

/// Debug output shows an integer's representation and kind, so equal
/// strings mean the values are the same, not only numerically equal.
fn exact(values: &[Value]) -> String {
    format!("{values:?}")
}

fn exact_list(list: &ListElements) -> String {
    exact(&list.to_vec())
}

/// The plain layout of `ListTestRecord`, a class only these tests use, so
/// registering it changes no other test's lists.
fn test_record_layout() -> &'static PlainLayout {
    static LAYOUT: OnceLock<&'static PlainLayout> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        let env = LayoutEnv {
            classes: [(
                "ListTestRecord".to_string(),
                ClassDefinition {
                    type_params: Vec::new(),
                    fields: vec![
                        ("x".to_string(), Type::named("int64")),
                        ("flag".to_string(), Type::named("bool")),
                        ("y".to_string(), Type::named("float32")),
                    ],
                },
            )]
            .into(),
            enums: Default::default(),
        };
        let descriptor = encode_descriptor(&Type::named("ListTestRecord"), &env).expect("a layout");
        plain_layout_for_descriptor(&descriptor).expect("the record has a plain layout")
    })
}

/// A `ListTestRecord`, whose layout is registered.
fn record(x: i64) -> Value {
    test_record_layout();
    Value::Instance(InstanceValue {
        class_name: "ListTestRecord".to_string(),
        fields: [
            ("x".to_string(), int(x)),
            ("flag".to_string(), Value::Bool(x % 2 == 0)),
            ("y".to_string(), Value::Float(x as f64 / 4.0)),
        ]
        .into(),
    })
}

/// A `ListTestRecord` without the registered shape, which no typed list
/// keeps.
fn misshapen_record(x: i64) -> Value {
    test_record_layout();
    Value::Instance(InstanceValue {
        class_name: "ListTestRecord".to_string(),
        fields: [("x".to_string(), Value::String(format!("r{x}")))].into(),
    })
}

#[test]
fn lists_keep_registered_records_in_place() {
    let layout = test_record_layout();
    assert_eq!(layout.stride_words(), 2);
    let values = vec![record(1), record(-2), record(3)];
    let list = ListElements::from(values.clone());
    assert_eq!(list.form(), ListForm::Typed(layout.access()));
    assert_eq!(exact_list(&list), exact(&values));
    let (form, elements, len, _) = list.view();
    assert_eq!(form, ListForm::Typed(layout.access()));
    assert_ne!(elements, 0);
    assert_eq!(len, 3);

    // A record of another shape, even under the same name, keeps values.
    let mixed = ListElements::from(vec![record(1), misshapen_record(2)]);
    assert_eq!(mixed.form(), ListForm::Values);
    let mut pushed = ListElements::new();
    pushed.push(record(4));
    assert_eq!(pushed.form(), ListForm::Typed(layout.access()));
    pushed.push(misshapen_record(5));
    assert_eq!(pushed.form(), ListForm::Values);
    assert_eq!(
        exact_list(&pushed),
        exact(&[record(4), misshapen_record(5)])
    );

    // Equal lists in either form compare and hash alike.
    let mut boxed = ListElements::from(values.clone());
    boxed.values_mut();
    assert_eq!(boxed.form(), ListForm::Values);
    assert_eq!(list, boxed);
    let typed_value = Value::Vec(crate::runtime_value::VecValue {
        element_type: Type::named("ListTestRecord"),
        elements: list.clone(),
        str_view: Default::default(),
    });
    let boxed_value = Value::Vec(crate::runtime_value::VecValue {
        element_type: Type::named("ListTestRecord"),
        elements: boxed,
        str_view: Default::default(),
    });
    assert_eq!(
        crate::collection_index::hash_value(&typed_value),
        crate::collection_index::hash_value(&boxed_value)
    );
    assert_eq!(typed_value.render(), boxed_value.render());
}

#[test]
fn lists_keep_scalars_only_when_every_element_fits_one_form() {
    let cases = [
        (vec![int(1), int(-2), int(i64::MIN)], ListForm::Int64),
        (
            vec![Value::Float(0.5), Value::Float(f64::NAN)],
            ListForm::Float64,
        ),
        (vec![Value::Bool(true), Value::Bool(false)], ListForm::Bool),
        (vec![], ListForm::Values),
        (vec![untyped(1), untyped(2)], ListForm::Values),
        (vec![int32(1), int32(i32::MIN)], ListForm::Int32),
        (
            vec![
                typed(-128, IntegerKind::Int8),
                typed(127, IntegerKind::Int8),
            ],
            ListForm::Int8,
        ),
        (vec![typed(-300, IntegerKind::Int16)], ListForm::Int16),
        (
            vec![typed(0, IntegerKind::Uint8), typed(255, IntegerKind::Uint8)],
            ListForm::Uint8,
        ),
        (vec![typed(65_535, IntegerKind::Uint16)], ListForm::Uint16),
        (
            vec![typed(4_294_967_295, IntegerKind::Uint32)],
            ListForm::Uint32,
        ),
        (
            vec![typed(u64::MAX.into(), IntegerKind::Uint64)],
            ListForm::Uint64,
        ),
        (vec![typed(1, IntegerKind::Int128)], ListForm::Values),
        (vec![int32(1), int(2)], ListForm::Values),
        (
            vec![typed(1, IntegerKind::Uint8), typed(1, IntegerKind::Int8)],
            ListForm::Values,
        ),
        (vec![int(1), untyped(2)], ListForm::Values),
        (vec![int(1), Value::Float(2.0)], ListForm::Values),
        (vec![Value::Float(1.0), Value::Bool(true)], ListForm::Values),
        (vec![Value::Bool(true), int(1)], ListForm::Values),
        (vec![Value::String("a".into())], ListForm::Values),
    ];
    for (values, form) in cases {
        let expected = exact(&values);
        let list = ListElements::from(values);
        assert_eq!(list.form(), form, "{expected}");
        assert_eq!(exact_list(&list), expected);
        assert_eq!(exact(&list.clone().into_vec()), expected);
    }
}

/// A small deterministic generator, so the operation sequences repeat.
struct Steps(u64);

impl Steps {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound.max(1)
    }
}

fn sample_value(steps: &mut Steps, family: usize) -> Value {
    // Most values match the family's form, so lists stay scalar for a
    // while before a mismatch moves them to the value form.
    let family = if steps.next(10) == 0 {
        steps.next(FAMILIES)
    } else {
        family
    };
    let small = steps.next(7) as i64 - 3;
    match family {
        0 => int(small),
        1 => Value::Float(small as f64 / 2.0),
        2 => Value::Bool(small > 0),
        3 => untyped(small),
        4 => Value::String(format!("s{small}")),
        5 => typed(small.into(), IntegerKind::Int8),
        6 => int32(small as i32 * 1_000_000),
        7 => typed((small + 3).into(), IntegerKind::Uint8),
        8 => typed((small + 3).into(), IntegerKind::Uint16),
        9 => typed(
            u128::from(u64::MAX - (small + 3) as u64) as i128,
            IntegerKind::Uint64,
        ),
        10 => record(small),
        _ => misshapen_record(small),
    }
}

/// The value families `sample_value` draws from.
const FAMILIES: usize = 12;

#[test]
fn every_edit_matches_a_plain_vector_of_values() {
    for seed in 0..144u64 {
        let mut steps = Steps(seed);
        let family = seed as usize % FAMILIES;
        let mut model: Vec<Value> = (0..steps.next(4))
            .map(|_| sample_value(&mut steps, family))
            .collect();
        let mut list = ListElements::from(model.clone());
        for _ in 0..80 {
            let len = model.len();
            match steps.next(16) {
                0 | 1 => {
                    let value = sample_value(&mut steps, family);
                    model.push(value.clone());
                    list.push(value);
                }
                2 => assert_eq!(exact(&model.pop().into_iter().collect::<Vec<_>>()), {
                    exact(&list.pop().into_iter().collect::<Vec<_>>())
                }),
                3 if len > 0 => {
                    let index = steps.next(len);
                    let value = sample_value(&mut steps, family);
                    let previous = std::mem::replace(&mut model[index], value.clone());
                    assert_eq!(exact(&[list.replace(index, value)]), exact(&[previous]));
                }
                4 => {
                    let index = steps.next(len + 1);
                    let value = sample_value(&mut steps, family);
                    model.insert(index, value.clone());
                    list.insert(index, value);
                }
                5 if len > 0 => {
                    let index = steps.next(len);
                    assert_eq!(exact(&[list.remove(index)]), exact(&[model.remove(index)]));
                }
                6 if len > 1 => {
                    let (first, second) = (steps.next(len), steps.next(len));
                    model.swap(first, second);
                    list.swap(first, second);
                }
                7 => {
                    model.reverse();
                    list.reverse();
                }
                8 => {
                    let keep = steps.next(len + 1);
                    model.truncate(keep);
                    list.truncate(keep);
                }
                9 => {
                    let more: Vec<Value> = (0..steps.next(3))
                        .map(|_| sample_value(&mut steps, family))
                        .collect();
                    model.extend(more.iter().cloned());
                    list.append(ListElements::from(more));
                }
                10 => {
                    let start = steps.next(len + 1);
                    let end = start + steps.next(len - start + 1);
                    let taken: Vec<Value> = model.drain(start..end).collect();
                    assert_eq!(exact_list(&list.take_range(start, end)), exact(&taken));
                }
                11 => {
                    let index = steps.next(len + 1);
                    let more: Vec<Value> = (0..steps.next(3))
                        .map(|_| sample_value(&mut steps, family))
                        .collect();
                    model.splice(index..index, more.iter().cloned());
                    list.insert_all(index, ListElements::from(more));
                }
                12 if steps.next(8) == 0 => {
                    model.clear();
                    list.clear();
                }
                // A lend moves a list of `Value`s to boxes, which every
                // later edit then works on.
                13 if len > 0 => {
                    let index = steps.next(len);
                    let lent = list.lend(index).map(|element| element.value().clone());
                    if let Some(lent) = lent {
                        assert_eq!(exact(&[lent]), exact(&model[index..=index]));
                        assert!(matches!(list, ListElements::Boxes(_)));
                    }
                }
                14 if len > 0 => {
                    let index = steps.next(len);
                    let value = sample_value(&mut steps, family);
                    model[index] = value.clone();
                    *list.get_mut(index).unwrap() = value;
                }
                // A copy shares the storage until either list changes.
                15 if len > 0 => {
                    let snapshot = list.clone();
                    let index = steps.next(len);
                    let value = sample_value(&mut steps, family);
                    let previous = std::mem::replace(&mut model[index], value.clone());
                    *list.last_mut().unwrap() = list.at(len - 1).into_owned();
                    *list.get_mut(index).unwrap() = value;
                    assert_eq!(
                        exact(&[snapshot.at(index).into_owned()]),
                        exact(&[previous])
                    );
                }
                _ => {}
            }
            assert_eq!(exact_list(&list), exact(&model), "seed {seed}");
            assert_eq!(list.len(), model.len());
            assert_eq!(list, model);
            for (index, value) in model.iter().enumerate() {
                assert_eq!(
                    exact(&[list.get(index).unwrap().into_owned()]),
                    exact(&[value.clone()])
                );
            }
            assert!(list.get(model.len()).is_none());
            if let Some(values) = list.value_refs() {
                assert_eq!(values.len(), model.len());
                assert_eq!(
                    exact(&values.rev().cloned().collect::<Vec<_>>()),
                    exact(&model.iter().rev().cloned().collect::<Vec<_>>())
                );
                assert_eq!(
                    exact(&list.value_at(0).into_iter().cloned().collect::<Vec<_>>()),
                    exact(&model[..model.len().min(1)])
                );
            } else {
                assert!(list.value_at(0).is_none());
            }
        }
    }
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn boxed_elements_join_a_list_in_their_own_boxes() {
    use crate::native_runtime::ElementBox;
    // An empty list, or one that keeps boxes, keeps the box it is given.
    let mut list = ListElements::new();
    let first = ElementBox::new(text("a"));
    let first_ptr = first.as_ptr();
    list.push_box(first);
    list.push_box(ElementBox::new(text("b")));
    let ListElements::Boxes(boxes) = &list else {
        panic!("the list keeps the boxes");
    };
    assert_eq!(boxes[0].as_ptr(), first_ptr);
    assert_eq!(list.to_vec(), vec![text("a"), text("b")]);
    // Any other form takes the value out of the box.
    let mut values = ListElements::from(vec![text("x")]);
    values.push_box(ElementBox::new(text("y")));
    assert!(matches!(values, ListElements::Values(_)));
    assert_eq!(values.to_vec(), vec![text("x"), text("y")]);
}

#[test]
fn lent_lists_keep_each_element_in_a_box_of_its_own() {
    let mut list = ListElements::from(vec![text("a"), text("b"), text("c")]);
    assert!(ListElements::from(vec![int(1)]).lend(0).is_none());
    let first = list.lend(1).unwrap().as_ptr();
    assert!(matches!(list, ListElements::Boxes(_)));
    // The box stays where it is while the list keeps it.
    assert_eq!(list.lend(1).unwrap().as_ptr(), first);
    assert!(list.lend(3).is_none());
    assert_eq!(
        list.view().0.access(),
        crate::native_runtime::DirectListAccess::Boxes as u32
    );
    // Unshared storage has room, which code that changes an element in
    // place checks.
    assert!(list.view().3 >= 3);
    assert_eq!(
        format!("{list:?}"),
        format!(
            "{:?}",
            SharedVec::from(vec![text("a"), text("b"), text("c")])
        )
    );

    // A copy shares the boxes until either list changes; a change copies
    // the storage, so the other list keeps its boxes.
    let copy = list.clone();
    list.push(text("d"));
    assert_eq!(copy.len(), 3);
    assert_eq!(list.len(), 4);
    let ListElements::Boxes(boxes) = &copy else {
        panic!("the copy keeps the boxes");
    };
    assert_eq!(boxes[1].as_ptr(), first);
    assert_ne!(list.lend(1).unwrap().as_ptr(), first);

    // Generated code reads only boxes no other list shares. Shared boxes
    // show as values, and a list lends from them only after taking boxes
    // of its own.
    let mut copy = copy;
    let other = copy.clone();
    assert_eq!(
        copy.view().0.access(),
        crate::native_runtime::DirectListAccess::Values as u32
    );
    assert_eq!(copy.view().3, 0);
    let own = copy.lend(1).unwrap().as_ptr();
    assert_ne!(own, first);
    assert_eq!(
        copy.view().0.access(),
        crate::native_runtime::DirectListAccess::Boxes as u32
    );
    let ListElements::Boxes(boxes) = &other else {
        panic!("the other list keeps the boxes");
    };
    assert_eq!(boxes[1].as_ptr(), first);
    assert_eq!(copy, other);

    // A list of `Value`s another list shares copies its elements into boxes,
    // and the other list keeps its `Value`s.
    let mut shared = ListElements::from(vec![text("x"), text("y")]);
    let other = shared.clone();
    assert_eq!(shared.lend(0).unwrap().value(), &text("x"));
    assert!(matches!(other, ListElements::Values(_)));
    assert_eq!(shared, other);

    // Lent lists sort, report their position order, and convert back.
    let mut words = ListElements::from(vec![text("c"), text("a"), text("b")]);
    words.lend(0);
    crate::list_sort::sort_elements(&mut words, false).unwrap();
    assert_eq!(
        exact_list(&words),
        exact(&[text("a"), text("b"), text("c")])
    );
    let mut keyed = ListElements::from(vec![int(1), int(2), int(3)]);
    crate::list_sort::sort_elements_by_keys(&mut keyed, &words, true).unwrap();
    assert_eq!(exact_list(&keyed), exact(&[int(3), int(2), int(1)]));
    let mut mixed = ListElements::from(vec![text("a"), int(1)]);
    mixed.lend(0);
    assert!(crate::list_sort::sort_elements(&mut mixed, false).is_err());
    assert!(crate::list_sort::sort_elements_by_keys(&mut keyed.clone(), &mixed, false).is_err());
    assert_eq!(
        exact(&words.into_vec()),
        exact(&[text("a"), text("b"), text("c")])
    );
}

#[test]
fn lent_lists_keep_their_boxes_through_edits_and_reads() {
    let boxed = |values: Vec<Value>| {
        let mut list = ListElements::from(values);
        list.lend(0);
        list
    };
    let mut list = boxed(vec![text("a"), text("d")]);
    list.append(boxed(vec![text("e")]));
    list.insert_all(1, boxed(vec![text("b"), text("c")]));
    assert_eq!(list.form(), ListForm::Boxes);
    // Generated code appends nothing in place to boxes.
    unsafe { list.adopt_appended(list.len()) };
    assert_eq!(
        exact(&list.value_refs().unwrap().cloned().collect::<Vec<_>>()),
        exact(&[text("a"), text("b"), text("c"), text("d"), text("e")])
    );
    assert!(ListElements::from(vec![int(1)]).value_refs().is_none());
    // An element edited in its box reads back through the edit.
    let element = list.get_mut(2).unwrap();
    assert_eq!(*element, text("c"));
    drop(element);
    let mut values = ListElements::from(vec![text("x")]);
    assert_eq!(*values.get_mut(0).unwrap(), text("x"));

    // Only the scalar forms a first element picks build a list directly,
    // but every form says which values it keeps.
    assert!(keeps(ListForm::Float32, &Value::Float(0.5)));
    assert!(!keeps(ListForm::Float32, &Value::Float(0.1)));
    assert!(keeps(ListForm::Boxes, &text("a")));
    let floats = ListElements::collect(ListForm::Float32, &[Value::Float(0.5)]).unwrap();
    assert_eq!(floats.form(), ListForm::Float32);
    let boxes = ListElements::collect(ListForm::Boxes, &[text("a")]).unwrap();
    assert_eq!(boxes.form(), ListForm::Boxes);
}

#[test]
fn scalar_lists_keep_their_form_through_whole_element_edits() {
    let mut list = ListElements::from(vec![int(1), int(2), int(3)]);
    list.push(int(4));
    list.insert(0, int(0));
    assert_eq!(exact_list(&list.take_range(1, 2)), exact(&[int(1)]));
    list.replace(0, int(9));
    list.remove(1);
    list.swap(0, 1);
    list.reverse();
    list.truncate(2);
    list.append(ListElements::from(vec![int(5)]));
    list.insert_all(1, ListElements::from(vec![int(6), int(7)]));
    assert_eq!(
        list.pop().map(|value| exact(&[value])),
        Some(exact(&[int(5)]))
    );
    assert_eq!(list.form(), ListForm::Int64);
    assert_eq!(exact_list(&list), exact(&[int(4), int(6), int(7), int(9)]));

    let mut floats = ListElements::from(vec![Value::Float(1.0)]);
    floats.push(Value::Float(2.0));
    floats.insert(0, Value::Float(0.0));
    floats.replace(2, Value::Float(3.0));
    assert_eq!(floats.form(), ListForm::Float64);
    let mut flags = ListElements::from(vec![Value::Bool(true)]);
    flags.push(Value::Bool(false));
    flags.insert(1, Value::Bool(true));
    flags.replace(0, Value::Bool(false));
    assert_eq!(flags.form(), ListForm::Bool);
    assert_eq!(
        exact_list(&flags),
        exact(&[Value::Bool(false), Value::Bool(true), Value::Bool(false)])
    );
}

#[test]
fn float32_lists_keep_four_byte_elements() {
    let single = |value: f32| Value::Float(f64::from(value));
    let mut list = ListElements::from(vec![Value::Float(1.5), Value::Float(0.1)]);
    assert_eq!(list.form(), ListForm::Float64);
    list.narrow_to_float32();
    assert_eq!(list.form(), ListForm::Float32);
    assert_eq!(exact_list(&list), exact(&[Value::Float(1.5), single(0.1)]));
    list.narrow_to_float32();
    assert_eq!(list.form(), ListForm::Float32);

    // Every float stored in the form is rounded, as a `float32` variable
    // rounds it.
    list.push(Value::Float(0.2));
    list.insert(0, Value::Float(0.3));
    list.replace(1, Value::Float(-0.4));
    list.append(ListElements::from(vec![
        Value::Float(0.5),
        Value::Float(0.6),
    ]));
    assert_eq!(list.form(), ListForm::Float32);
    assert_eq!(
        exact_list(&list),
        exact(&[
            single(0.3),
            single(-0.4),
            single(0.1),
            single(0.2),
            single(0.5),
            single(0.6),
        ])
    );
    // Only a float that is exactly a `float32` is found.
    assert!(list.contains(&single(0.1)));
    assert!(!list.contains(&Value::Float(0.1)));
    assert_eq!(list, ListElements::from(list.to_vec()));
    assert_eq!(
        exact_list(&list.take_range(4, 6)),
        exact(&[single(0.5), single(0.6)])
    );
    assert_eq!(list.form(), ListForm::Float32);
    // Another float list takes the widened elements.
    let mut other = ListElements::from(vec![Value::Float(2.0)]);
    other.append(list.clone());
    assert_eq!(other.form(), ListForm::Float64);
    assert_eq!(
        exact_list(&other),
        exact(&[
            Value::Float(2.0),
            single(0.3),
            single(-0.4),
            single(0.1),
            single(0.2)
        ])
    );

    // A non-float value moves the list to the value form.
    list.push(Value::String("text".into()));
    assert_eq!(list.form(), ListForm::Values);

    // A list of floats or an empty list narrows. Any other list keeps its
    // form.
    let mut floats = ListElements::from(vec![Value::Float(0.1)]);
    floats.values_mut();
    floats.narrow_to_float32();
    assert_eq!(floats.form(), ListForm::Float32);
    assert_eq!(exact_list(&floats), exact(&[single(0.1)]));
    let mut empty = ListElements::new();
    empty.narrow_to_float32();
    assert_eq!(empty.form(), ListForm::Float32);
    empty.append(ListElements::from(vec![Value::Float(0.7)]));
    assert_eq!(empty.form(), ListForm::Float32);
    assert_eq!(exact_list(&empty), exact(&[single(0.7)]));
    let mut mixed = ListElements::from(vec![Value::Float(0.1), int(1)]);
    mixed.narrow_to_float32();
    assert_eq!(mixed.form(), ListForm::Values);
    let mut integers = ListElements::from(vec![int(1)]);
    integers.narrow_to_float32();
    assert_eq!(integers.form(), ListForm::Int64);
}

#[test]
fn an_empty_list_takes_the_form_of_its_first_element() {
    let mut list = ListElements::new();
    list.push(Value::Float(1.5));
    assert_eq!(list.form(), ListForm::Float64);
    list.clear();
    assert_eq!(list.form(), ListForm::Values);
    list.insert(0, Value::Bool(true));
    assert_eq!(list.form(), ListForm::Bool);
    list.truncate(0);
    list.push(Value::String("text".into()));
    assert_eq!(list.form(), ListForm::Values);

    let mut emptied = ListElements::from(vec![int(1)]);
    emptied.pop();
    emptied.push(Value::String("text".into()));
    assert_eq!(emptied.form(), ListForm::Values);
    let mut appended = ListElements::new();
    appended.append(ListElements::from(vec![int(1)]));
    assert_eq!(appended.form(), ListForm::Int64);
    let mut spliced = ListElements::new();
    spliced.insert_all(0, ListElements::from(vec![Value::Bool(true)]));
    assert_eq!(spliced.form(), ListForm::Bool);
    spliced.insert_all(1, ListElements::new());
    assert_eq!(spliced.len(), 1);
}

#[test]
fn a_mismatched_element_moves_the_list_to_the_value_form() {
    let mut list = ListElements::from(vec![int(1), int(2)]);
    list.push(untyped(3));
    assert_eq!(list.form(), ListForm::Values);
    assert_eq!(exact_list(&list), exact(&[int(1), int(2), untyped(3)]));

    let mut replaced = ListElements::from(vec![Value::Float(1.0)]);
    replaced.replace(0, int(1));
    assert_eq!(replaced.form(), ListForm::Values);
    let mut inserted = ListElements::from(vec![Value::Bool(true)]);
    inserted.insert(1, Value::Float(1.0));
    assert_eq!(inserted.form(), ListForm::Values);
    let mut appended = ListElements::from(vec![int(1)]);
    appended.append(ListElements::from(vec![Value::Bool(true)]));
    assert_eq!(appended.form(), ListForm::Values);
    assert_eq!(exact_list(&appended), exact(&[int(1), Value::Bool(true)]));
}

#[test]
fn edits_inside_elements_use_the_value_form_and_update_picks_again() {
    let mut list = ListElements::from(vec![int(1), int(2)]);
    assert!(list.get_mut(2).is_none());
    assert_eq!(list.form(), ListForm::Int64);
    *list.get_mut(0).unwrap() = int(5);
    assert_eq!(list.form(), ListForm::Values);
    assert_eq!(exact_list(&list), exact(&[int(5), int(2)]));
    *list.last_mut().unwrap() = int(6);
    list.update(|values| values.sort_by_key(|value| format!("{value:?}")));
    assert_eq!(list.form(), ListForm::Int64);
    assert_eq!(exact_list(&list), exact(&[int(5), int(6)]));
    assert!(ListElements::new().last_mut().is_none());
    assert!(list.as_values().is_none());
    assert_eq!(list.values_mut().len(), 2);
    assert_eq!(list.as_values().map(<[Value]>::len), Some(2));
}

#[test]
fn clones_share_storage_until_a_write() {
    for values in [
        vec![int(1), int(2)],
        vec![Value::Float(1.0)],
        vec![Value::Bool(true)],
        vec![Value::String("a".into())],
    ] {
        let list = ListElements::from(values);
        let mut copy = list.clone();
        assert!(list.is_shared() && copy.is_shared());
        assert_eq!(list.as_ptr(), copy.as_ptr());
        copy.swap(0, 0);
        assert!(!list.is_shared() && !copy.is_shared());
        assert_ne!(list.as_ptr(), copy.as_ptr());
        assert_eq!(list, copy);
    }
}

#[test]
fn equality_ignores_the_form() {
    let scalar = ListElements::from(vec![int(1), int(2)]);
    let mut boxed = ListElements::from(vec![int(1), int(2)]);
    boxed.values_mut();
    assert_eq!(boxed.form(), ListForm::Values);
    assert_eq!(scalar, boxed);
    assert_ne!(scalar, ListElements::from(vec![int(1)]));
    assert_ne!(scalar, ListElements::from(vec![int(1), int(3)]));
    let nan = ListElements::from(vec![Value::Float(f64::NAN)]);
    assert_ne!(nan, nan.clone());
    let flags = ListElements::from(vec![Value::Bool(true)]);
    assert_eq!(flags, flags.clone());
    assert_ne!(flags, ListElements::from(vec![int(1)]));
}

#[test]
fn reads_iterate_in_both_directions_and_report_their_length() {
    for values in [
        vec![int(1), int(2), int(3)],
        vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)],
        vec![Value::Bool(true), Value::Bool(false), Value::Bool(true)],
        vec![untyped(1), untyped(2), untyped(3)],
        vec![
            typed(1, IntegerKind::Uint8),
            typed(2, IntegerKind::Uint8),
            typed(3, IntegerKind::Uint8),
        ],
        vec![text("a"), text("b"), text("c")],
    ] {
        let mut list = ListElements::from(values.clone());
        // Strings read from their boxes once a list lends one.
        if matches!(values[0], Value::String(_)) {
            list.lend(0);
            assert_eq!(list.form(), ListForm::Boxes);
        }
        // `skip` steps over elements without building them.
        assert_eq!(
            exact(&list.iter().skip(1).map(Cow::into_owned).collect::<Vec<_>>()),
            exact(&values[1..])
        );
        assert!(list.iter().nth(3).is_none());
        let mut iter = list.iter();
        assert_eq!(iter.len(), 3);
        assert_eq!(iter.size_hint(), (3, Some(3)));
        assert_eq!(
            exact(&[iter.next_back().unwrap().into_owned()]),
            exact(&values[2..])
        );
        assert_eq!(
            exact(&[iter.next().unwrap().into_owned()]),
            exact(&values[..1])
        );
        assert_eq!(iter.len(), 1);
        assert_eq!(
            exact(
                &(&list)
                    .into_iter()
                    .rev()
                    .map(Cow::into_owned)
                    .collect::<Vec<_>>()
            ),
            exact(&values.iter().rev().cloned().collect::<Vec<_>>())
        );
        assert_eq!(
            exact(&[list.first().unwrap().into_owned()]),
            exact(&values[..1])
        );
        assert_eq!(
            exact(&[list.last().unwrap().into_owned()]),
            exact(&values[2..])
        );
        assert_eq!(
            exact(&list.clone().into_iter().collect::<Vec<_>>()),
            exact(&values)
        );
        assert!(list.contains(&values[1]));
        assert!(!list.contains(&Value::String("x".into())));

        let mut iter = list.iter();
        assert_eq!(
            exact(&[iter.nth(1).unwrap().into_owned()]),
            exact(&values[1..2])
        );
        assert!(iter.clone().nth(1).is_none());
        assert_eq!(
            exact(&[iter.nth_back(0).unwrap().into_owned()]),
            exact(&values[2..])
        );
        assert!(iter.nth_back(0).is_none());
        let mut iter = list.iter();
        assert_eq!(
            exact(&[iter.nth_back(2).unwrap().into_owned()]),
            exact(&values[..1])
        );
        assert!(iter.nth(0).is_none());
        assert_eq!(
            exact(&list.iter().skip(2).map(Cow::into_owned).collect::<Vec<_>>()),
            exact(&values[2..])
        );
    }
    assert!(ListElements::new().last().is_none());
    let ints = ListElements::from(vec![int(4)]);
    assert!(ints.contains(&untyped(4)));
    assert!(!ints.contains(&untyped(5)));
    assert!(!ints.contains(&Value::Int(IntegerValue::from_u64(u64::MAX))));
    assert_eq!(
        exact_list(&[int(1), int(2)].into_iter().collect()),
        exact(&[int(1), int(2)])
    );
    assert_eq!(
        exact_list(&ListElements::from(SharedVec::from(vec![int(3)]))),
        exact(&[int(3)])
    );
}

#[test]
fn debug_output_does_not_depend_on_the_form() {
    let scalar = ListElements::from(vec![int(1)]);
    let mut boxed = scalar.clone();
    boxed.values_mut();
    assert_eq!(format!("{scalar:?}"), format!("{boxed:?}"));
    assert_eq!(
        format!("{scalar:?}"),
        format!("{:?}", SharedVec::from(vec![int(1)]))
    );
}

#[test]
#[should_panic(expected = "list replace index 1 is out of range for length 1")]
fn replace_past_the_end_panics() {
    ListElements::from(vec![int(1)]).replace(1, int(2));
}

#[test]
#[should_panic(expected = "list insert index 2 is out of range for length 1")]
fn insert_past_the_end_panics() {
    ListElements::from(vec![int(1)]).insert(2, int(2));
}

#[test]
#[should_panic(expected = "list remove index 1 is out of range for length 1")]
fn remove_past_the_end_panics() {
    ListElements::from(vec![int(1)]).remove(1);
}

#[test]
#[should_panic(expected = "list range 1..3 is out of range for length 2")]
fn take_range_past_the_end_panics() {
    ListElements::from(vec![int(1), int(2)]).take_range(1, 3);
}

#[test]
#[should_panic(expected = "list insert index 3 is out of range for length 1")]
fn insert_all_past_the_end_panics() {
    ListElements::from(vec![int(1)]).insert_all(3, ListElements::from(vec![int(2)]));
}

#[test]
#[should_panic(expected = "list index 2 is out of range for length 2")]
fn at_past_the_end_panics() {
    let list = ListElements::from(vec![int(1), int(2)]);
    assert_eq!(exact(&[list.at(1).into_owned()]), exact(&[int(2)]));
    list.at(2);
}

#[test]
fn shuffle_gives_the_same_order_in_every_form() {
    let lists = [
        (1..=40).map(int).collect::<Vec<_>>(),
        (1..=40).map(|n| Value::Float(n as f64)).collect(),
        (1..=40).map(|n| Value::Bool(n % 3 == 0)).collect(),
        (1..=40).map(record).collect(),
    ];
    for values in lists {
        let mut scalars = ListElements::from(values.clone());
        assert_ne!(scalars.form(), ListForm::Values);
        let mut boxed = ListElements::from(values.clone());
        boxed.values_mut();
        assert_eq!(boxed.form(), ListForm::Values);

        scalars.shuffle(&mut DeterministicRng::from_seed(7));
        boxed.shuffle(&mut DeterministicRng::from_seed(7));
        assert_eq!(exact_list(&scalars), exact_list(&boxed));
        assert_ne!(scalars, values, "seed 7 should move some element");
    }
}

#[test]
fn reserve_and_capacity_work_in_every_form() {
    let lists = [
        vec![untyped(1)],
        vec![int(1)],
        vec![Value::Float(1.0)],
        vec![Value::Bool(true)],
        vec![record(1)],
    ];
    for values in lists {
        let mut list = ListElements::from(values);
        assert!(!list.is_empty());
        list.try_reserve(64)
            .expect("a small reserve should succeed");
        assert!(list.capacity() >= 65);
        list.clear();
        assert!(list.is_empty());
    }
    assert!(ListElements::new().is_empty());
}

#[test]
fn slices_of_records_in_place_copy_their_words() {
    let layout = test_record_layout();
    let vector = crate::runtime_value::VecValue {
        element_type: Type::named("ListTestRecord"),
        elements: ListElements::from(vec![record(1), record(2), record(3), record(4)]),
        str_view: Default::default(),
    };
    let slice = crate::runtime_value::slice_vec_owned(&vector, Some(1), Some(3))
        .expect("the slice is in range");
    assert_eq!(slice.elements.form(), ListForm::Typed(layout.access()));
    assert_eq!(exact_list(&slice.elements), exact(&[record(2), record(3)]));
    assert_eq!(vector.elements.len(), 4);
}

#[test]
fn records_in_place_iterate_from_either_end() {
    let layout = test_record_layout();
    let values: Vec<Value> = (1..=5).map(record).collect();
    let list = ListElements::from(values.clone());
    assert_eq!(list.form(), ListForm::Typed(layout.access()));
    let mut iter = list.iter();
    assert_eq!(iter.nth(1).map(Cow::into_owned), Some(values[1].clone()));
    assert_eq!(
        iter.next_back().map(Cow::into_owned),
        Some(values[4].clone())
    );
    assert_eq!(
        iter.nth_back(1).map(Cow::into_owned),
        Some(values[2].clone())
    );
    assert_eq!(iter.len(), 0);
    assert!(iter.next_back().is_none());
    assert!(list.iter().nth(9).is_none());
    assert!(list.iter().nth_back(9).is_none());
    let reversed: Vec<Value> = list.iter().rev().map(Cow::into_owned).collect();
    assert_eq!(
        exact(&reversed),
        exact(&values.iter().rev().cloned().collect::<Vec<_>>())
    );
}

#[test]
fn records_in_place_sort_only_by_positions() {
    let layout = test_record_layout();
    let mut list = ListElements::from(vec![record(1), record(2), record(3)]);
    assert_eq!(list.form(), ListForm::Typed(layout.access()));

    // Records have no built-in order, as elements or as keys.
    let unorderable = "list sort found values without one built-in order";
    let error = crate::list_sort::sort_elements(&mut list, false).unwrap_err();
    assert!(error.to_string().contains(unorderable), "{error}");
    let keys = list.clone();
    let error = crate::list_sort::sort_elements_by_keys(&mut list, &keys, true).unwrap_err();
    assert!(error.to_string().contains(unorderable), "{error}");

    // The positions a user `Ord` sort computes move the records in place.
    let order = ListElements::from(vec![int(2), int(0), int(1)]);
    crate::list_sort::permute_elements(&mut list, &order).expect("a permutation");
    assert_eq!(list.form(), ListForm::Typed(layout.access()));
    assert_eq!(exact_list(&list), exact(&[record(3), record(1), record(2)]));
}
