use super::*;
use crate::integer::IntegerKind;

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn untyped(value: i64) -> Value {
    Value::Int(IntegerValue::from_signed(i128::from(value)))
}

fn int32(value: i32) -> Value {
    Value::Int(
        IntegerValue::from_typed_signed(i128::from(value), IntegerKind::Int32)
            .expect("an i32 fits int32"),
    )
}

/// Debug output shows an integer's representation and kind, so equal
/// strings mean the values are the same, not only numerically equal.
fn exact(values: &[Value]) -> String {
    format!("{values:?}")
}

fn exact_list(list: &ListElements) -> String {
    exact(&list.to_vec())
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
        (vec![int32(1)], ListForm::Values),
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

#[test]
fn int64_bits_accepts_only_values_from_i64_rebuilds() {
    assert_eq!(int64_bits(IntegerValue::from_i64(-7)), Some(-7));
    assert_eq!(int64_bits(IntegerValue::from_i64(i64::MAX)), Some(i64::MAX));
    assert_eq!(int64_bits(IntegerValue::from_signed(3)), None);
    assert_eq!(int64_bits(IntegerValue::from_u64(3)), None);
    let int128 = IntegerValue::from_typed_signed(1 << 80, IntegerKind::Int128).unwrap();
    assert_eq!(int64_bits(int128), None);
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
        steps.next(5)
    } else {
        family
    };
    let small = steps.next(7) as i64 - 3;
    match family {
        0 => int(small),
        1 => Value::Float(small as f64 / 2.0),
        2 => Value::Bool(small > 0),
        3 => untyped(small),
        _ => Value::String(format!("s{small}")),
    }
}

#[test]
fn every_edit_matches_a_plain_vector_of_values() {
    for seed in 0..60u64 {
        let mut steps = Steps(seed);
        let family = (seed % 5) as usize;
        let mut model: Vec<Value> = (0..steps.next(4))
            .map(|_| sample_value(&mut steps, family))
            .collect();
        let mut list = ListElements::from(model.clone());
        for _ in 0..80 {
            let len = model.len();
            match steps.next(13) {
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
        }
    }
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
    ] {
        let list = ListElements::from(values.clone());
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
