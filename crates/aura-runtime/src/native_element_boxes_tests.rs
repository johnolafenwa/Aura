use crate::list_elements::ListElements;
use crate::native_runtime::{aura_direct_release_value, aura_direct_retain_value, value_ref};
use crate::runtime_value::Value;

use super::ElementBox;

fn text(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn element_boxes_compare_print_and_copy_their_values() {
    let element = ElementBox::new(text("a"));
    let copy = element.clone();
    assert_ne!(copy.as_ptr(), element.as_ptr());
    assert_eq!(copy, element);
    assert_eq!(format!("{element:?}"), format!("{:?}", text("a")));
    let mut list = ElementBox::new(Value::Vec(crate::runtime_value::VecValue::new(
        aura_core::types::Type::named("int64"),
        ListElements::new(),
    )));
    if let Value::Vec(vector) = &mut *list.value_mut() {
        vector
            .elements
            .push(Value::Int(crate::integer::IntegerValue::from_i64(4)));
    }
    // The guard brought the box's list view up to date, and the list has
    // room while no other value shares its storage.
    let room = |list: &ElementBox| unsafe {
        (*list.as_ptr())
            .list_capacity
            .load(std::sync::atomic::Ordering::Relaxed)
    };
    assert_eq!(unsafe { (*list.as_ptr()).list_len.get() }, 1);
    assert!(room(&list) > 0);
    // A read of the box may copy its value, which shares the storage, so
    // the box gives the room back.
    let shared = list.value().clone();
    assert_eq!(room(&list), 0);
    drop(shared);
    assert_eq!(copy.into_value(), text("a"));
}

#[test]
fn element_boxes_copy_an_element_another_reference_holds_before_a_change() {
    let mut list = ListElements::from(vec![text("a"), text("b")]);
    let ptr = list.lend(0).unwrap().as_ptr();
    // Generated code copies an element it keeps, but should anything take
    // a reference to a box, a change to the element leaves it the old one.
    unsafe { aura_direct_retain_value(ptr) };
    *list.get_mut(0).unwrap() = text("changed");
    assert_ne!(list.lend(0).unwrap().as_ptr(), ptr);
    assert_eq!(unsafe { value_ref(ptr) }, text("a"));
    // A list dropped while another reference holds an element only drops
    // its own reference, and moving out copies a shared element.
    let mut kept = ListElements::from(vec![text("k")]);
    let kept_ptr = kept.lend(0).unwrap().as_ptr();
    unsafe { aura_direct_retain_value(kept_ptr) };
    assert_eq!(kept.into_vec(), vec![text("k")]);
    assert_eq!(unsafe { value_ref(kept_ptr) }, text("k"));
    unsafe {
        aura_direct_release_value(ptr);
        aura_direct_release_value(kept_ptr);
    }
    assert_eq!(crate::list_elements::ValueRefs::default().len(), 0);
}

fn int(value: i64) -> Value {
    Value::Int(crate::integer::IntegerValue::from_i64(value))
}

/// A box that holds a list of `int64` with `values`, as a list of lists or
/// a list field holds one, with room for more.
fn int_list_box(values: &[i64]) -> ElementBox {
    let mut list = ElementBox::new(Value::Vec(crate::runtime_value::VecValue::new(
        aura_core::types::Type::named("int64"),
        ListElements::new(),
    )));
    if let Value::Vec(vector) = &mut *list.value_mut() {
        for value in values {
            vector.elements.push(int(*value));
        }
        // Eight elements pushed and popped leave room for as many.
        for _ in 0..8 {
            vector.elements.push(int(0));
        }
        for _ in 0..8 {
            vector.elements.pop();
        }
    }
    list
}

/// Appends an `int64` to the list in `list` the way generated code does
/// while the list view has room: past the end, raising only the view's
/// length.
fn append_in_place(list: &ElementBox, value: i64) {
    let layout = crate::native_runtime::direct_list_layout();
    let base = list.as_ptr() as *mut u8;
    unsafe {
        let access = *(base.add(layout.access_offset as usize) as *const u32);
        assert_eq!(
            access,
            crate::native_runtime::DirectListAccess::Int64 as u32
        );
        let len = *(base.add(layout.len_offset as usize) as *const usize);
        let room = *(base.add(layout.capacity_offset as usize) as *const u32) as usize;
        assert!(len < room, "the list has no room: {len} of {room}");
        let elements = *(base.add(layout.elements_offset as usize) as *const usize) as *mut i64;
        *elements.add(len) = value;
        *(base.add(layout.len_offset as usize) as *mut usize) = len + 1;
    }
}

fn ints(value: &Value) -> Vec<Value> {
    match value {
        Value::Vec(vector) => vector
            .elements
            .iter()
            .map(|value| value.into_owned())
            .collect(),
        other => panic!("expected a list, found {other:?}"),
    }
}

#[test]
fn element_boxes_take_in_the_elements_generated_code_appended() {
    // A read through the holder sees them, and gives the room back.
    let list = int_list_box(&[1]);
    append_in_place(&list, 2);
    append_in_place(&list, 3);
    assert_eq!(ints(list.value()), vec![int(1), int(2), int(3)]);
    assert_eq!(unsafe { (*list.as_ptr()).list_len.get() }, 3);
    // So does a copy of the box, and a comparison.
    let mut list = int_list_box(&[1]);
    append_in_place(&list, 2);
    let copy = list.clone();
    assert_eq!(ints(copy.value()), vec![int(1), int(2)]);
    assert_eq!(copy, list);
    drop(copy);
    // A change through the holder starts after them, and gives the list
    // its room back.
    if let Value::Vec(vector) = &mut *list.value_mut() {
        vector.elements.push(int(3));
    }
    append_in_place(&list, 4);
    if let Value::Vec(vector) = &mut *list.value_mut() {
        vector.elements.push(int(5));
    }
    assert_eq!(
        ints(list.value()),
        vec![int(1), int(2), int(3), int(4), int(5)]
    );
    if let Value::Vec(vector) = &mut *list.value_mut() {
        vector.elements.truncate(3);
        vector.elements.push(int(4));
    }
    // A holder that refreshes the box's view after its own change keeps
    // them, and so does one that moves the value out.
    append_in_place(&list, 5);
    list.refresh_view();
    assert_eq!(unsafe { (*list.as_ptr()).list_len.get() }, 5);
    append_in_place(&list, 6);
    assert_eq!(
        ints(&list.into_value()),
        vec![int(1), int(2), int(3), int(4), int(5), int(6)]
    );
    // A box dropped with elements still to take in frees its storage.
    let list = int_list_box(&[]);
    append_in_place(&list, 7);
    drop(list);
}

#[test]
fn element_boxes_take_appended_elements_in_once_for_readers_on_several_threads() {
    // A holder may become shared while a box still has elements to take
    // in. Its readers then come at once, under the holder's read lock.
    for _ in 0..64 {
        let list = int_list_box(&[1]);
        for value in 2..=6 {
            append_in_place(&list, value);
        }
        let list = &list;
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(move || {
                    assert_eq!(ints(list.value()).len(), 6);
                });
            }
        });
        assert_eq!(ints(list.value()).last(), Some(&int(6)));
    }
}
