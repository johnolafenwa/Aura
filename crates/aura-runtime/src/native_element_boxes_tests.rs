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
