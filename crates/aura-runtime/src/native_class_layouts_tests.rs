//! Class instances in their typed layouts (typed-layout step 2e): the
//! layouts, the fields of one instance, and the helpers generated code and
//! the rest of the runtime reach them through.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use aura_core::layout::{encode_descriptor, ClassDefinition, LayoutEnv};
use aura_core::types::Type;

use super::{register_class_layout, ClassLayout, ClassLayoutCell, TypedFields, FIRST_CLASS_ACCESS};
use crate::collection_index::hash_value;
use crate::integer::{IntegerKind, IntegerValue};
use crate::list_elements::ListElements;
use crate::native_runtime::{
    aura_direct_element_path_load, aura_direct_element_path_store, aura_direct_instance_copy_field,
    aura_direct_instance_get_field, aura_direct_instance_new_typed, aura_direct_instance_put_field,
    aura_direct_instance_read_plain, aura_direct_instance_restore_field,
    aura_direct_instance_set_field, aura_direct_instance_set_field_owned,
    aura_direct_instance_take_field, aura_direct_register_plain_layouts, aura_direct_release_value,
    aura_direct_retain_value, aura_direct_vec_push_in_place, boxed_value, list_view_of,
    register_plain_layout, share_direct_value, value_mut, value_ref,
    with_direct_task_runtime_scope, with_task_runtime_error_capture, DirectPlainLayoutEntry,
    ElementBox, OpaqueValue, UNREGISTERED_PLAIN_ACCESS,
};
use crate::runtime_value::{
    run_lightweight_root_task, InstanceFields, InstanceValue, TupleValue, Value, VecValue,
};

/// The class codes these tests register under, far above the codes the
/// code generator gives classes.
const FIRST_TEST_ACCESS: u32 = FIRST_CLASS_ACCESS + 0x1000_0000;

fn int64(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn int32(value: i64) -> Value {
    Value::Int(
        IntegerValue::from_typed_signed(i128::from(value), IntegerKind::Int32)
            .expect("the value fits int32"),
    )
}

fn ints(values: &[i64]) -> Value {
    Value::Vec(VecValue {
        element_type: Type::named("int64"),
        elements: ListElements::from(values.iter().map(|value| int64(*value)).collect::<Vec<_>>()),
        str_view: Default::default(),
    })
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

/// `Book` (with `prefix`) holds a title, a nested `Meta`, a tuple, a list,
/// and scalars, so its block has boxes, a class held inline, and padding.
fn book_env(prefix: &str) -> LayoutEnv {
    let meta = format!("{prefix}Meta");
    let class = |fields: Vec<(&str, Type)>| ClassDefinition {
        type_params: Vec::new(),
        fields: fields
            .into_iter()
            .map(|(name, ty)| (name.to_string(), ty))
            .collect(),
    };
    LayoutEnv {
        classes: [
            (
                format!("{prefix}Book"),
                class(vec![
                    ("title", Type::named("str")),
                    ("meta", Type::named(&meta)),
                    (
                        "pos",
                        Type::Tuple(vec![Type::named("int64"), Type::named("int64")]),
                    ),
                    (
                        "items",
                        Type::Named("list".to_string(), vec![Type::named("int64")]),
                    ),
                    ("small", Type::named("int32")),
                    ("ratio", Type::named("float32")),
                    ("flag", Type::named("bool")),
                ]),
            ),
            (
                meta,
                class(vec![
                    ("name", Type::named("str")),
                    ("count", Type::named("int64")),
                ]),
            ),
        ]
        .into(),
        enums: Default::default(),
    }
}

fn descriptor(prefix: &str, class: &str) -> &'static str {
    Box::leak(
        encode_descriptor(&Type::named(&format!("{prefix}{class}")), &book_env(prefix))
            .into_boxed_str(),
    )
}

fn layout(prefix: &str, access: u32) -> &'static ClassLayout {
    register_class_layout(descriptor(prefix, "Book"), access).expect("the class is laid out")
}

fn meta(prefix: &str, name: &str, count: i64) -> Value {
    Value::Instance(InstanceValue {
        class_name: format!("{prefix}Meta"),
        fields: BTreeMap::from([
            ("name".to_string(), text(name)),
            ("count".to_string(), int64(count)),
        ])
        .into(),
    })
}

fn book_fields(prefix: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("title".to_string(), text("t")),
        ("meta".to_string(), meta(prefix, "m", 1)),
        (
            "pos".to_string(),
            Value::Tuple(TupleValue {
                element_types: vec![Type::named("int64"), Type::named("int64")],
                elements: vec![int64(3), int64(4)],
            }),
        ),
        ("items".to_string(), ints(&[1, 2])),
        ("small".to_string(), int32(7)),
        ("ratio".to_string(), Value::Float(0.5)),
        ("flag".to_string(), Value::Bool(true)),
    ])
}

fn typed_book(prefix: &str, access: u32) -> TypedFields {
    TypedFields::from_map(layout(prefix, access), book_fields(prefix)).expect("every field fits")
}

fn instance(class_name: &str, fields: InstanceFields) -> Value {
    Value::Instance(InstanceValue {
        class_name: class_name.to_string(),
        fields,
    })
}

/// The message of the runtime error `work` stops with.
fn runtime_failure(work: impl FnOnce() + Send + 'static) -> String {
    run_lightweight_root_task(move || {
        with_direct_task_runtime_scope(|| {
            Ok(with_task_runtime_error_capture(|| {
                work();
                Value::Unit
            }))
        })
    })
    .expect_err("the call should stop the program")
    .message
}

/// Runs `work` in a task, as generated code runs.
fn in_task(work: impl FnOnce() + Send + 'static) {
    run_lightweight_root_task(move || {
        with_direct_task_runtime_scope(|| {
            work();
            Ok(Value::Unit)
        })
    })
    .expect("the work runs");
}

#[test]
fn classes_register_once_under_one_code() {
    let first = layout("RegisterOnce", FIRST_TEST_ACCESS);
    assert_eq!(first.access(), FIRST_TEST_ACCESS);
    assert_eq!(first.class_name(), "RegisterOnceBook");
    assert!(format!("{first:?}").contains("RegisterOnceBook"));
    assert!(std::ptr::eq(
        first,
        layout("RegisterOnce", FIRST_TEST_ACCESS)
    ));
    let elsewhere =
        register_class_layout(descriptor("RegisterOnce", "Book"), FIRST_TEST_ACCESS + 1)
            .expect_err("one layout has one code");
    assert!(
        elsewhere.contains("registered under access code"),
        "{elsewhere}"
    );
    let taken = register_class_layout(descriptor("RegisterOnce", "Meta"), FIRST_TEST_ACCESS)
        .expect_err("one code names one layout");
    assert!(taken.contains("names another layout"), "{taken}");
    for code in [FIRST_CLASS_ACCESS - 1, UNREGISTERED_PLAIN_ACCESS] {
        let refused = register_class_layout(descriptor("RegisterOnce", "Meta"), code)
            .expect_err("not a class code");
        assert!(
            refused.contains("is not a class layout access code"),
            "{refused}"
        );
    }
    // Plain layouts never take class codes.
    let pair = encode_descriptor(
        &Type::Tuple(vec![Type::named("int64"), Type::named("int64")]),
        &LayoutEnv::default(),
    );
    let Err(plain) = register_plain_layout(&pair, Some(FIRST_CLASS_ACCESS)) else {
        panic!("a class code");
    };
    assert!(
        plain.contains("is not a plain layout access code"),
        "{plain}"
    );
}

#[test]
fn only_classes_without_parameters_or_recursion_keep_their_fields_in_place() {
    let env = LayoutEnv {
        classes: [
            (
                "PlaceGeneric".to_string(),
                ClassDefinition {
                    type_params: vec!["T".to_string()],
                    fields: vec![("value".to_string(), Type::named("T"))],
                },
            ),
            (
                "PlaceNode".to_string(),
                ClassDefinition {
                    type_params: Vec::new(),
                    fields: vec![(
                        "next".to_string(),
                        Type::normalize_union(
                            vec![Type::named("PlaceNode"), Type::Unit],
                            "main",
                            &Default::default(),
                        )
                        .expect("a valid union"),
                    )],
                },
            ),
        ]
        .into(),
        enums: Default::default(),
    };
    let cases = [
        (
            Type::Named("PlaceGeneric".to_string(), vec![Type::named("int64")]),
            "generic class",
        ),
        (Type::named("PlaceNode"), "recursive class"),
        (Type::named("int64"), "is not a class"),
    ];
    for (index, (ty, expected)) in cases.into_iter().enumerate() {
        let descriptor = encode_descriptor(&ty, &env);
        let refused = register_class_layout(&descriptor, FIRST_TEST_ACCESS + 0x100 + index as u32)
            .expect_err("kept in a map");
        assert!(refused.contains(expected), "{refused}");
    }
}

#[test]
fn cells_hold_the_layout_once_registered() {
    let cell: &'static ClassLayoutCell = Box::leak(Box::default());
    let unregistered = unsafe { super::registered_class_layout(cell) }.expect_err("empty");
    assert!(
        unregistered.contains("before it was registered"),
        "{unregistered}"
    );
    let null = unsafe { super::registered_class_layout(std::ptr::null()) }.expect_err("null");
    assert!(null.contains("null layout cell"), "{null}");
    let null = unsafe { super::celled_class_layout(std::ptr::null(), b"", FIRST_TEST_ACCESS) }
        .expect_err("null");
    assert!(null.contains("null layout cell"), "{null}");
    let not_text = unsafe { super::celled_class_layout(cell, &[0xff], FIRST_TEST_ACCESS + 2) }
        .expect_err("not UTF-8");
    assert!(not_text.contains("not UTF-8"), "{not_text}");
    let descriptor = descriptor("Celled", "Book");
    let entry = DirectPlainLayoutEntry {
        cell: std::ptr::from_ref(cell).cast(),
        descriptor: descriptor.as_ptr(),
        len: descriptor.len(),
        access: u64::from(FIRST_TEST_ACCESS + 3),
    };
    // A class code in the plain layout table registers a class layout.
    unsafe { aura_direct_register_plain_layouts(&entry, 1) };
    let registered = unsafe { super::registered_class_layout(cell) }.expect("filled");
    assert_eq!(registered.access(), FIRST_TEST_ACCESS + 3);
    assert_eq!(cell.access.load(Ordering::Acquire), FIRST_TEST_ACCESS + 3);
    // A filled cell is read without the descriptor.
    let again = unsafe { super::celled_class_layout(cell, b"", 0) }.expect("filled");
    assert!(std::ptr::eq(registered, again));
    let failure = runtime_failure(move || {
        let entry = DirectPlainLayoutEntry {
            cell: Box::leak(Box::new(ClassLayoutCell::new())) as *const ClassLayoutCell as *const _,
            descriptor: b"?".as_ptr(),
            len: 1,
            access: u64::from(FIRST_TEST_ACCESS + 4),
        };
        unsafe { aura_direct_register_plain_layouts(&entry, 1) };
    });
    assert!(failure.contains("invalid plain layout"), "{failure}");
}

#[test]
fn typed_fields_read_as_their_map_reads() {
    let prefix = "Reads";
    let typed = typed_book(prefix, FIRST_TEST_ACCESS + 10);
    let map = book_fields(prefix);
    assert_eq!(typed.len(), map.len());
    assert!(!typed.is_empty());
    assert!(typed.contains_key("meta") && !typed.contains_key("missing"));
    assert_eq!(
        typed.keys().collect::<Vec<_>>(),
        map.keys().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(
        typed
            .iter()
            .map(|(name, value)| (name.to_string(), value.into_owned()))
            .collect::<Vec<_>>(),
        map.clone().into_iter().collect::<Vec<_>>()
    );
    assert_eq!(
        typed.iter().next_back().map(|(name, _)| name),
        Some("title")
    );
    // A box lends its value; an inline field is read out of the block.
    assert!(
        matches!(typed.get("title"), Some(Cow::Borrowed(Value::String(title))) if title == "t")
    );
    assert!(matches!(typed.get("small"), Some(Cow::Owned(_))));
    assert_eq!(typed.get("missing"), None);
    assert_eq!(
        typed.get("meta").map(Cow::into_owned),
        Some(meta(prefix, "m", 1))
    );
    assert_eq!(format!("{typed:?}"), format!("{map:?}"));

    // Equal to its map form, with the same hash and the same rendering.
    let name = format!("{prefix}Book");
    let typed_value = instance(&name, InstanceFields::Typed(typed.clone()));
    let map_value = instance(&name, map.clone().into());
    assert_eq!(typed_value, map_value);
    assert_eq!(hash_value(&typed_value), hash_value(&map_value));
    assert_eq!(typed_value.render(), map_value.render());
    let InstanceFields::Typed(copy) = InstanceFields::Typed(typed.clone()) else {
        unreachable!();
    };
    assert_eq!(copy.into_map(), map);

    // A map that does not fit comes back.
    let mut wrong = book_fields(prefix);
    wrong.insert("small".to_string(), text("seven"));
    let back = TypedFields::from_map(layout(prefix, FIRST_TEST_ACCESS + 10), wrong.clone())
        .expect_err("a str is not an int32");
    assert_eq!(back, wrong);
    let mut short = book_fields(prefix);
    short.remove("flag");
    assert!(TypedFields::from_map(layout(prefix, FIRST_TEST_ACCESS + 10), short).is_err());
}

#[test]
fn typed_fields_change_where_they_lie() {
    let prefix = "Writes";
    let mut typed = typed_book(prefix, FIRST_TEST_ACCESS + 20);
    typed.insert("small", int32(9)).expect("an int32 fits");
    typed.insert("title", text("u")).expect("a str fits");
    assert_eq!(typed.get("small").map(Cow::into_owned), Some(int32(9)));
    assert_eq!(typed.get("title").map(Cow::into_owned), Some(text("u")));
    let refused = typed
        .insert("small", text("x"))
        .expect_err("a str is not an int32");
    assert!(refused.contains("does not fit"), "{refused}");
    let missing = typed
        .insert("missing", int64(1))
        .expect_err("no such field");
    assert!(missing.contains("has no field `missing`"), "{missing}");
    assert!(typed.with_field_mut("missing", |_| ()).is_err());

    // A box changes in place; an inline field is moved out for the edit and
    // written back.
    typed
        .with_field_mut("items", |items| {
            let Value::Vec(items) = items else {
                panic!("a list");
            };
            items.elements.push(int64(3));
        })
        .expect("a field");
    assert_eq!(
        typed.get("items").map(Cow::into_owned),
        Some(ints(&[1, 2, 3]))
    );
    typed
        .with_field_mut("meta", |meta| {
            let Value::Instance(meta) = meta else {
                panic!("an instance");
            };
            meta.fields
                .insert("count", int64(5))
                .expect("a map takes any value");
        })
        .expect("a field");
    assert_eq!(
        typed.get("meta").map(Cow::into_owned),
        Some(meta(prefix, "m", 5))
    );
    // An edit that moves the value out leaves the field zeroed.
    typed
        .with_field_mut("small", |small| std::mem::replace(small, Value::Unit))
        .expect("a field");
    assert_eq!(typed.get("small").map(Cow::into_owned), Some(int32(0)));

    // Paths reach through classes and tuples held inline.
    assert_eq!(typed.take_path(&["meta", "count"]), Some(Some(int64(5))));
    assert_eq!(
        typed.get("meta").map(Cow::into_owned),
        Some(meta(prefix, "m", 0))
    );
    typed
        .set_path(&["pos", "1"], int64(8))
        .expect("inside")
        .expect("fits");
    assert_eq!(typed.take_path(&["pos", "1"]), Some(Some(int64(8))));
    assert!(typed
        .set_path(&["meta", "name"], text("n"))
        .expect("inside")
        .is_ok());
    // A path that goes on inside a box leaves the block.
    assert_eq!(typed.take_path(&["items", "0"]), None);
    assert_eq!(typed.set_path(&["items", "0"], int64(1)), Err(int64(1)));
    assert_eq!(typed.take_path(&["missing"]), None);

    // A moved-out box reads as missing, and a write fills it again.
    assert_eq!(typed.remove("title"), Some(text("u")));
    assert_eq!(typed.remove("title"), None);
    assert!(!typed.contains_key("title"));
    assert_eq!(typed.len(), 6);
    assert!(typed.iter().all(|(name, _)| name != "title"));
    let moved = typed
        .with_field_mut("title", |_| ())
        .expect_err("moved out");
    assert!(moved.contains("used after it moved out"), "{moved}");
    assert_eq!(typed.take_path(&["title"]), Some(None));
    assert_eq!(typed.remove("missing"), None);
    let copy = typed.clone();
    assert!(!copy.contains_key("title"));
    assert!(copy.into_map().get("title").is_none());
    typed.insert("title", text("v")).expect("fills the slot");
    assert_eq!(typed.get("title").map(Cow::into_owned), Some(text("v")));

    // Every field moved out leaves nothing to drop.
    let mut empty = TypedFields::new(typed.layout());
    for name in ["title", "items"] {
        assert_eq!(empty.remove(name), None);
    }
    assert_eq!(empty.len(), 5);
    assert_eq!(empty.get("small").map(Cow::into_owned), Some(int32(0)));
}

#[test]
fn a_typed_instance_box_shows_its_block_while_confined() {
    let prefix = "Views";
    let typed = typed_book(prefix, FIRST_TEST_ACCESS + 30);
    let data = typed.data() as usize;
    let value = instance(&format!("{prefix}Book"), InstanceFields::Typed(typed));
    assert_eq!(list_view_of(&value), (FIRST_TEST_ACCESS + 30, data, 0, 0));
    let boxed = boxed_value(value);
    let header = unsafe { &*boxed };
    assert_eq!(
        header.list_access.load(Ordering::Relaxed),
        FIRST_TEST_ACCESS + 30
    );
    assert_eq!(header.list_elements.get(), data);
    // A change through the instance refreshes its field boxes' views too.
    unsafe {
        value_mut(boxed, |value| {
            let Value::Instance(book) = value else {
                panic!("an instance");
            };
            book.fields
                .insert("items", ints(&[1, 2, 3, 4]))
                .expect("a list fits");
        })
    };
    let Value::Instance(book) = (unsafe { &*header.value.get() }) else {
        panic!("an instance");
    };
    let items = book
        .fields
        .typed()
        .expect("typed")
        .get("items")
        .expect("present");
    let Cow::Borrowed(items) = items else {
        panic!("a box lends its value");
    };
    assert_eq!(*items, ints(&[1, 2, 3, 4]));
    // A map instance shows no view, and neither does a shared box.
    let map_value = instance("ViewsMap", book_fields(prefix).into());
    assert_eq!(list_view_of(&map_value).0, 0);
    unsafe { share_direct_value(boxed) };
    assert_eq!(header.list_access.load(Ordering::Relaxed), 0);
}

#[test]
fn generated_code_builds_and_writes_typed_instances() {
    in_task(|| {
        let prefix = "Built";
        let layout = layout(prefix, FIRST_TEST_ACCESS + 40);
        let cell: &'static ClassLayoutCell = Box::leak(Box::default());
        cell.access.store(layout.access(), Ordering::Release);
        cell.layout
            .store(std::ptr::from_ref(layout).cast_mut(), Ordering::Release);
        let built = unsafe { aura_direct_instance_new_typed(cell) };
        let header = unsafe { &*built };
        let block = header.list_elements.get();
        let slot = |name: &str| {
            let field = layout.field(name).expect("a field");
            (block + field.offset) as *mut Option<ElementBox>
        };
        // A box with one reference is adopted as it is.
        let title = boxed_value(text("t"));
        unsafe { aura_direct_instance_put_field(slot("title"), title) };
        assert_eq!(
            unsafe { (*slot("title")).as_ref() }.map(ElementBox::as_ptr),
            Some(title)
        );
        // A box another reference holds is copied, and the reference kept.
        let items = boxed_value(ints(&[1]));
        unsafe { aura_direct_retain_value(items) };
        unsafe { aura_direct_instance_put_field(slot("items"), items) };
        let held = unsafe { (*slot("items")).as_ref() }.map(ElementBox::as_ptr);
        assert_ne!(held, Some(items));
        assert_eq!(unsafe { value_ref(items) }, ints(&[1]));
        unsafe { aura_direct_release_value(items) };
        // A new box replaces the one the slot held.
        let replaced = boxed_value(ints(&[2]));
        unsafe { aura_direct_instance_put_field(slot("items"), replaced) };
        let Value::Instance(book) = (unsafe { value_ref(built) }) else {
            panic!("an instance");
        };
        assert_eq!(
            book.fields.get("items").map(Cow::into_owned),
            Some(ints(&[2]))
        );
        assert_eq!(
            book.fields.get("title").map(Cow::into_owned),
            Some(text("t"))
        );

        // Restoring the box a lend left alone keeps it; a shared one is
        // copied for the field; another box is adopted.
        let lent = unsafe { (*slot("items")).as_ref() }
            .map(ElementBox::as_ptr)
            .unwrap();
        unsafe { aura_direct_instance_restore_field(slot("items"), lent) };
        assert_eq!(
            unsafe { (*slot("items")).as_ref() }.map(ElementBox::as_ptr),
            Some(lent)
        );
        unsafe { aura_direct_retain_value(lent) };
        unsafe { aura_direct_instance_restore_field(slot("items"), lent) };
        assert_ne!(
            unsafe { (*slot("items")).as_ref() }.map(ElementBox::as_ptr),
            Some(lent)
        );
        unsafe { aura_direct_release_value(lent) };
        let other = boxed_value(ints(&[3]));
        unsafe { aura_direct_instance_restore_field(slot("items"), other) };
        assert_eq!(
            unsafe { (*slot("items")).as_ref() }.map(ElementBox::as_ptr),
            Some(other)
        );
        // A read of a handle field copies the value out of the field box.
        let copied = aura_direct_instance_copy_field(other);
        assert_ne!(copied, other);
        assert_eq!(unsafe { value_ref(copied) }, ints(&[3]));
        assert_eq!(unsafe { value_ref(other) }, ints(&[3]));
        unsafe { aura_direct_release_value(copied) };
        unsafe { aura_direct_release_value(built) };
    });
    let failure = runtime_failure(|| {
        let mut slot = None;
        unsafe { aura_direct_instance_put_field(&mut slot, std::ptr::null_mut()) };
    });
    assert!(failure.contains("null field value"), "{failure}");
    let failure = runtime_failure(|| {
        let mut slot = None;
        unsafe { aura_direct_instance_restore_field(&mut slot, std::ptr::null_mut()) };
    });
    assert!(failure.contains("null field value"), "{failure}");
    let failure = runtime_failure(|| {
        let cell: &'static ClassLayoutCell = Box::leak(Box::default());
        unsafe { aura_direct_instance_new_typed(cell) };
    });
    assert!(failure.contains("before it was registered"), "{failure}");
}

/// A boxed typed `Book` with `prefix`, for the runtime's field helpers.
fn boxed_book(prefix: &str, access: u32) -> *mut OpaqueValue {
    boxed_value(instance(
        &format!("{prefix}Book"),
        InstanceFields::Typed(typed_book(prefix, access)),
    ))
}

fn field_of(boxed: *mut OpaqueValue, name: &str) -> Value {
    let Value::Instance(book) = (unsafe { value_ref(boxed) }) else {
        panic!("an instance");
    };
    book.fields.get(name).map(Cow::into_owned).expect("present")
}

#[test]
fn appended_typed_instances_keep_their_own_boxes() {
    in_task(|| {
        let prefix = "Appended";
        let access = FIRST_TEST_ACCESS + 70;
        let list = boxed_value(Value::Vec(VecValue {
            element_type: Type::named(&format!("{prefix}Book")),
            elements: ListElements::new(),
            str_view: Default::default(),
        }));
        let push = |element: *mut OpaqueValue| aura_direct_vec_push_in_place(list, element);
        let boxes = || match unsafe { value_ref(list) } {
            Value::Vec(VecValue {
                elements: ListElements::Boxes(boxes),
                ..
            }) => boxes.iter().map(ElementBox::as_ptr).collect::<Vec<_>>(),
            other => panic!("the list keeps boxes, found {other:?}"),
        };
        // An instance with one reference joins the list in its own box.
        let first = boxed_book(prefix, access);
        push(first);
        assert_eq!(boxes(), vec![first]);
        // One another reference holds is copied into a box of the list's.
        let second = boxed_book(prefix, access);
        unsafe { aura_direct_retain_value(second) };
        push(second);
        let held = boxes();
        assert_eq!(held.len(), 2);
        assert_ne!(held[1], second);
        assert_eq!(unsafe { value_ref(second) }, unsafe { value_ref(held[1]) });
        unsafe { aura_direct_release_value(second) };
        // A list that keeps `Value`s takes the instance out of its box.
        let values = boxed_value(Value::Vec(VecValue {
            element_type: Type::named(&format!("{prefix}Book")),
            elements: ListElements::from(vec![instance(
                &format!("{prefix}Book"),
                InstanceFields::Typed(typed_book(prefix, access)),
            )]),
            str_view: Default::default(),
        }));
        aura_direct_vec_push_in_place(values, boxed_book(prefix, access));
        let Value::Vec(kept) = (unsafe { value_ref(values) }) else {
            panic!("a list");
        };
        assert!(matches!(kept.elements, ListElements::Values(_)));
        assert_eq!(kept.elements.len(), 2);
        unsafe {
            aura_direct_release_value(values);
            aura_direct_release_value(list);
        }
    });
}

#[test]
fn runtime_field_helpers_reach_typed_instances() {
    in_task(|| {
        let prefix = "Helpers";
        let book = boxed_book(prefix, FIRST_TEST_ACCESS + 50);
        let read = |path: &str| unsafe {
            let field = aura_direct_instance_get_field(book, path.as_ptr(), path.len());
            let value = value_ref(field);
            aura_direct_release_value(field);
            value
        };
        assert_eq!(read("small"), int32(7));
        assert_eq!(read("title"), text("t"));

        // A nested path is taken and set inside the block.
        let path = "meta.count";
        let taken = aura_direct_instance_take_field(book, path.as_ptr(), path.len());
        assert_eq!(unsafe { value_ref(taken) }, int64(1));
        aura_direct_instance_set_field_owned(book, path.as_ptr(), path.len(), taken);
        assert_eq!(field_of(book, "meta"), meta(prefix, "m", 1));
        // A path inside a box goes on there.
        let path = "items";
        let taken = aura_direct_instance_take_field(book, path.as_ptr(), path.len());
        assert_eq!(unsafe { value_ref(taken) }, ints(&[1, 2]));
        aura_direct_instance_set_field_owned(book, path.as_ptr(), path.len(), taken);
        let path = "title";
        let replacement = boxed_value(text("w"));
        aura_direct_instance_set_field_owned(book, path.as_ptr(), path.len(), replacement);
        assert_eq!(field_of(book, "title"), text("w"));

        // A plain field reads into its bytes.
        let point = register_plain_layout(
            Box::leak(
                encode_descriptor(
                    &Type::Tuple(vec![Type::named("int64"), Type::named("int64")]),
                    &LayoutEnv::default(),
                )
                .into_boxed_str(),
            ),
            None,
        )
        .expect("a plain tuple");
        let cell = Box::leak(Box::new(crate::native_runtime::PlainLayoutCell::new()));
        let _ = point;
        let descriptor = encode_descriptor(
            &Type::Tuple(vec![Type::named("int64"), Type::named("int64")]),
            &LayoutEnv::default(),
        );
        unsafe {
            crate::native_runtime::celled_plain_layout(cell, descriptor.as_bytes(), point.access())
        }
        .expect("registered");
        let mut words = [0u64; 2];
        let field = "pos";
        unsafe {
            aura_direct_instance_read_plain(
                book,
                field.as_ptr(),
                field.len(),
                words.as_mut_ptr().cast(),
                cell,
            )
        };
        assert_eq!(words, [3, 4]);

        // `set_field` returns a new instance with the field replaced.
        let field = "small";
        let small = boxed_value(int32(11));
        let updated = aura_direct_instance_set_field(book, field.as_ptr(), field.len(), small);
        unsafe { aura_direct_release_value(small) };
        assert_eq!(field_of(updated, "small"), int32(11));
        assert_eq!(field_of(book, "small"), int32(7));

        // Element paths read and write through the block.
        let path = "items.[i]";
        let selectors = [1i64];
        let element = aura_direct_element_path_load(
            book,
            path.as_ptr(),
            path.len(),
            selectors.as_ptr(),
            1,
            0,
            0,
        );
        assert_eq!(unsafe { value_ref(element) }, int64(2));
        unsafe { aura_direct_release_value(element) };
        let path = "meta.count";
        let count = aura_direct_element_path_load(
            book,
            path.as_ptr(),
            path.len(),
            [0i64; 0].as_ptr(),
            0,
            0,
            0,
        );
        assert_eq!(unsafe { value_ref(count) }, int64(1));
        unsafe { aura_direct_release_value(count) };
        let path = "items.[i]";
        let stored = aura_direct_element_path_store(
            book,
            path.as_ptr(),
            path.len(),
            selectors.as_ptr(),
            1,
            boxed_value(int64(20)),
            0,
            0,
        );
        unsafe { aura_direct_release_value(stored) };
        assert_eq!(field_of(book, "items"), ints(&[1, 20]));
        let path = "meta.count";
        let stored = aura_direct_element_path_store(
            book,
            path.as_ptr(),
            path.len(),
            [0i64; 0].as_ptr(),
            0,
            boxed_value(int64(30)),
            0,
            0,
        );
        unsafe { aura_direct_release_value(stored) };
        assert_eq!(field_of(book, "meta"), meta(prefix, "m", 30));
        for value in [book, updated] {
            unsafe { aura_direct_release_value(value) };
        }
    });
    let failure = runtime_failure(|| {
        let book = boxed_book("HelpersMissing", FIRST_TEST_ACCESS + 51);
        let path = "missing.count";
        aura_direct_instance_take_field(book, path.as_ptr(), path.len());
    });
    assert!(failure.contains("has no field `missing`"), "{failure}");
    let failure = runtime_failure(|| {
        let book = boxed_book("HelpersMoved", FIRST_TEST_ACCESS + 52);
        let path = "title";
        let taken = aura_direct_instance_take_field(book, path.as_ptr(), path.len());
        unsafe { aura_direct_release_value(taken) };
        aura_direct_instance_take_field(book, path.as_ptr(), path.len());
    });
    assert!(failure.contains("has no field `title`"), "{failure}");
    let failure = runtime_failure(|| {
        let book = boxed_book("HelpersWrong", FIRST_TEST_ACCESS + 53);
        let path = "small";
        aura_direct_instance_set_field_owned(
            book,
            path.as_ptr(),
            path.len(),
            boxed_value(text("x")),
        );
    });
    assert!(failure.contains("does not fit"), "{failure}");
}

#[test]
fn an_instance_turns_its_layout_into_a_map_when_asked() {
    let prefix = "ToMap";
    let mut fields = InstanceFields::Typed(typed_book(prefix, FIRST_TEST_ACCESS + 60));
    assert!(fields.typed().is_some());
    fields.map_mut().insert("extra".to_string(), int64(1));
    assert!(fields.typed().is_none());
    assert_eq!(fields.map().len(), 8);
    assert_eq!(fields.keys().next(), Some("extra"));
    assert_eq!(fields.values().count(), 8);
    assert!(!fields.is_empty());
    assert!(format!("{fields:?}").contains("extra"));
    let typed = InstanceFields::Typed(typed_book(prefix, FIRST_TEST_ACCESS + 60));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| typed.map().len()))
        .expect_err("typed");
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.contains("keeps its fields in its layout"),
        "{message}"
    );
    assert_eq!(
        typed.iter().next_back().map(|(name, _)| name),
        Some("title")
    );
    let map = InstanceFields::default();
    assert!(map.is_empty());
    assert_eq!(map.iter().next_back(), None);
}
