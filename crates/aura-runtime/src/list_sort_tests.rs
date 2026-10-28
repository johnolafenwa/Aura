use super::*;
use crate::integer::{IntegerKind, IntegerValue};

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn int32(value: i32) -> Value {
    Value::Int(
        IntegerValue::from_typed_signed(i128::from(value), IntegerKind::Int32)
            .expect("an i32 fits int32"),
    )
}

fn text(value: &str) -> Value {
    Value::String(value.to_string().into())
}

fn list(values: Vec<Value>) -> ListElements {
    values.into()
}

fn rendered(elements: &ListElements) -> Vec<String> {
    elements
        .iter()
        .map(|value| match value.as_ref() {
            Value::Int(value) => value.to_string(),
            Value::Float(value) => format!("{value:?}"),
            Value::String(value) => value.to_string(),
            Value::Duration(value) => format!("{value}ns"),
            Value::Bool(value) => value.to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn int64_lists_sort_in_both_directions() {
    let mut values = list(vec![int(3), int(-1), int(2), int(-1), int(0)]);
    sort_elements(&mut values, false).unwrap();
    assert_eq!(rendered(&values), ["-1", "-1", "0", "2", "3"]);
    sort_elements(&mut values, true).unwrap();
    assert_eq!(rendered(&values), ["3", "2", "0", "-1", "-1"]);
}

#[test]
fn float_lists_put_nan_last_and_keep_signed_zeros_in_input_order() {
    let mut values = list(vec![
        Value::Float(2.0),
        Value::Float(f64::NAN),
        Value::Float(0.0),
        Value::Float(-1.5),
        Value::Float(-0.0),
        Value::Float(f64::INFINITY),
    ]);
    sort_elements(&mut values, false).unwrap();
    assert_eq!(
        rendered(&values),
        ["-1.5", "0.0", "-0.0", "2.0", "inf", "NaN"]
    );
    sort_elements(&mut values, true).unwrap();
    assert_eq!(
        rendered(&values),
        ["NaN", "inf", "2.0", "0.0", "-0.0", "-1.5"]
    );
}

#[test]
fn float32_lists_sort_and_order_by_float32_keys() {
    let floats = |values: &[f64]| {
        let mut elements = list(values.iter().map(|value| Value::Float(*value)).collect());
        elements.narrow_to_float32();
        assert!(matches!(elements, ListElements::Float32(_)));
        elements
    };
    let mut values = floats(&[2.0, f64::NAN, -1.5, 0.5]);
    sort_elements(&mut values, false).unwrap();
    assert_eq!(rendered(&values), ["-1.5", "0.5", "2.0", "NaN"]);
    sort_elements(&mut values, true).unwrap();
    assert_eq!(rendered(&values), ["NaN", "2.0", "0.5", "-1.5"]);
    assert!(matches!(values, ListElements::Float32(_)));

    let mut names = list(vec![text("a"), text("b"), text("c")]);
    sort_elements_by_keys(&mut names, &floats(&[1.0, -1.0, 1.0]), false).unwrap();
    assert_eq!(rendered(&names), ["b", "a", "c"]);
}

#[test]
fn float_sort_order_is_total() {
    assert_eq!(float_sort_order(1.0, 2.0), Ordering::Less);
    assert_eq!(float_sort_order(f64::NAN, 2.0), Ordering::Greater);
    assert_eq!(float_sort_order(2.0, f64::NAN), Ordering::Less);
    assert_eq!(float_sort_order(f64::NAN, f64::NAN), Ordering::Equal);
    assert_eq!(float_sort_order(-0.0, 0.0), Ordering::Equal);
}

#[test]
fn str_lists_sort_by_utf8_bytes() {
    let mut values = list(vec![
        text("pear"),
        text("Pear"),
        text("é"),
        text("apple"),
        text(""),
        text("z"),
        text("app"),
    ]);
    sort_elements(&mut values, false).unwrap();
    assert_eq!(
        rendered(&values),
        ["", "Pear", "app", "apple", "pear", "z", "é"]
    );
    sort_elements(&mut values, true).unwrap();
    assert_eq!(
        rendered(&values),
        ["é", "z", "pear", "apple", "app", "Pear", ""]
    );
}

#[test]
fn boxed_integers_and_durations_sort() {
    let mut values = list(vec![int32(5), int32(-7), int32(5), int32(1)]);
    sort_elements(&mut values, false).unwrap();
    assert_eq!(rendered(&values), ["-7", "1", "5", "5"]);
    let mut durations = list(vec![
        Value::Duration(30),
        Value::Duration(-10),
        Value::Duration(20),
    ]);
    sort_elements(&mut durations, true).unwrap();
    assert_eq!(rendered(&durations), ["30ns", "20ns", "-10ns"]);
}

#[test]
fn short_lists_are_already_sorted() {
    let mut empty = ListElements::new();
    sort_elements(&mut empty, false).unwrap();
    assert!(empty.is_empty());
    let mut flags = list(vec![Value::Bool(true)]);
    sort_elements(&mut flags, false).unwrap();
    assert_eq!(rendered(&flags), ["true"]);
}

#[test]
fn values_without_a_built_in_order_are_an_error() {
    let mut flags = list(vec![Value::Bool(true), Value::Bool(false)]);
    assert!(sort_elements(&mut flags, false)
        .unwrap_err()
        .message
        .contains("cannot order `bool` values"));
    let mut mixed = list(vec![int32(1), text("a")]);
    assert!(sort_elements(&mut mixed, false)
        .unwrap_err()
        .message
        .contains("without one built-in order"));
    let mut units = list(vec![Value::Unit, Value::Unit]);
    assert!(sort_elements(&mut units, false).is_err());
}

fn typed(value: i64, kind: IntegerKind) -> Value {
    Value::Int(
        IntegerValue::from_signed(i128::from(value))
            .with_runtime_kind(kind)
            .expect("the test value fits its kind"),
    )
}

#[test]
fn every_unboxed_integer_form_sorts_and_orders_keys() {
    let kinds = [
        IntegerKind::Int8,
        IntegerKind::Int16,
        IntegerKind::Int32,
        IntegerKind::Uint8,
        IntegerKind::Uint16,
        IntegerKind::Uint32,
        IntegerKind::Uint64,
    ];
    for kind in kinds {
        let mut values = list([3, 0, 2, 0, 1].map(|value| typed(value, kind)).to_vec());
        let form = values.form();
        assert_ne!(form, crate::list_elements::ListForm::Values, "{kind:?}");
        sort_elements(&mut values, false).unwrap();
        assert_eq!(rendered(&values), ["0", "0", "1", "2", "3"], "{kind:?}");
        sort_elements(&mut values, true).unwrap();
        assert_eq!(rendered(&values), ["3", "2", "1", "0", "0"], "{kind:?}");
        assert_eq!(values.form(), form, "{kind:?}");

        let keys = list([2, 1, 2, 1, 0].map(|value| typed(value, kind)).to_vec());
        let names = || list(vec![text("a"), text("b"), text("c"), text("d"), text("e")]);
        let mut sorted = names();
        sort_elements_by_keys(&mut sorted, &keys, false).unwrap();
        assert_eq!(rendered(&sorted), ["e", "b", "d", "a", "c"], "{kind:?}");
        let mut sorted = names();
        sort_elements_by_keys(&mut sorted, &keys, true).unwrap();
        assert_eq!(rendered(&sorted), ["a", "c", "b", "d", "e"], "{kind:?}");
    }
}

#[test]
fn key_sorts_are_stable_in_both_directions() {
    let names = || list(vec![text("a"), text("b"), text("c"), text("d"), text("e")]);
    let int_keys = list(vec![int(2), int(1), int(2), int(1), int(0)]);
    let mut sorted = names();
    sort_elements_by_keys(&mut sorted, &int_keys, false).unwrap();
    assert_eq!(rendered(&sorted), ["e", "b", "d", "a", "c"]);
    let mut sorted = names();
    sort_elements_by_keys(&mut sorted, &int_keys, true).unwrap();
    assert_eq!(rendered(&sorted), ["a", "c", "b", "d", "e"]);

    let float_keys = list(vec![
        Value::Float(f64::NAN),
        Value::Float(1.0),
        Value::Float(f64::NAN),
        Value::Float(-1.0),
        Value::Float(1.0),
    ]);
    let mut sorted = names();
    sort_elements_by_keys(&mut sorted, &float_keys, false).unwrap();
    assert_eq!(rendered(&sorted), ["d", "b", "e", "a", "c"]);
    let mut sorted = names();
    sort_elements_by_keys(&mut sorted, &float_keys, true).unwrap();
    assert_eq!(rendered(&sorted), ["a", "c", "b", "e", "d"]);

    let str_keys = list(vec![text("x"), text("b"), text("x"), text("a"), text("b")]);
    let mut sorted = names();
    sort_elements_by_keys(&mut sorted, &str_keys, false).unwrap();
    assert_eq!(rendered(&sorted), ["d", "b", "e", "a", "c"]);
    let mut sorted = names();
    sort_elements_by_keys(&mut sorted, &str_keys, true).unwrap();
    assert_eq!(rendered(&sorted), ["a", "c", "b", "e", "d"]);
}

#[test]
fn key_sorts_move_scalar_elements_and_keep_their_form() {
    let mut values = list(vec![int(10), int(20), int(30)]);
    let keys = list(vec![text("c"), text("a"), text("b")]);
    sort_elements_by_keys(&mut values, &keys, false).unwrap();
    assert_eq!(rendered(&values), ["20", "30", "10"]);
    assert!(matches!(values, ListElements::Int64(_)));
}

#[test]
fn key_sorts_check_their_keys() {
    let mut values = list(vec![int(1), int(2)]);
    let short = list(vec![int(1)]);
    assert!(sort_elements_by_keys(&mut values, &short, false)
        .unwrap_err()
        .message
        .contains("1 keys for 2 elements"));
    let flags = list(vec![Value::Bool(true), Value::Bool(false)]);
    assert!(sort_elements_by_keys(&mut values, &flags, false)
        .unwrap_err()
        .message
        .contains("cannot order `bool` keys"));
    let mixed = list(vec![int(1), text("a")]);
    assert!(sort_elements_by_keys(&mut values, &mixed, false).is_err());
    let mut one = list(vec![int(1)]);
    sort_elements_by_keys(&mut one, &list(vec![text("a")]), false).unwrap();
    assert_eq!(rendered(&one), ["1"]);
}

#[test]
fn a_shared_list_is_copied_before_it_is_sorted() {
    let original = list(vec![int(3), int(1), int(2)]);
    let mut sorted = original.clone();
    sort_elements(&mut sorted, false).unwrap();
    assert_eq!(rendered(&original), ["3", "1", "2"]);
    assert_eq!(rendered(&sorted), ["1", "2", "3"]);

    let original = list(vec![text("c"), text("a"), text("b")]);
    let mut sorted = original.clone();
    sort_elements_by_keys(&mut sorted, &list(vec![int(2), int(0), int(1)]), false).unwrap();
    assert_eq!(rendered(&original), ["c", "a", "b"]);
    assert_eq!(rendered(&sorted), ["a", "b", "c"]);
}

#[test]
fn permutations_follow_every_cycle() {
    let mut values = list((0..7).map(|value| text(&value.to_string())).collect());
    // Two cycles, (0 3 5) and (1 6), and fixed points 2 and 4.
    let order = list([3, 6, 2, 5, 4, 0, 1].into_iter().map(int).collect());
    permute_elements(&mut values, &order).unwrap();
    assert_eq!(rendered(&values), ["3", "6", "2", "5", "4", "0", "1"]);
}

#[test]
fn permutations_reject_positions_that_are_not_one_permutation() {
    let mut values = list(vec![int(1), int(2), int(3)]);
    for order in [
        vec![int(0), int(1)],
        vec![int(0), int(1), int(1)],
        vec![int(0), int(1), int(3)],
        vec![int(0), int(1), int(-1)],
        vec![int(0), int(1), text("2")],
    ] {
        assert!(permute_elements(&mut values, &list(order))
            .unwrap_err()
            .message
            .contains("not a permutation"));
    }
    assert_eq!(rendered(&values), ["1", "2", "3"]);
}

#[test]
fn large_lists_sort_like_the_standard_stable_sort() {
    let mut state = 41_i64;
    let mut keys = Vec::new();
    for _ in 0..10_000 {
        state = state * 48271 % 2147483647;
        keys.push(state % 100);
    }
    let mut values = list((0..keys.len() as i64).map(int).collect());
    let key_list = list(keys.iter().copied().map(int).collect());
    sort_elements_by_keys(&mut values, &key_list, false).unwrap();
    let mut expected = (0..keys.len()).collect::<Vec<_>>();
    expected.sort_by_key(|position| keys[*position]);
    let expected = expected
        .into_iter()
        .map(|position| position.to_string())
        .collect::<Vec<_>>();
    assert_eq!(rendered(&values), expected);
}
