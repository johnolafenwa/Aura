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
    aura_direct_instance_take_field, aura_direct_register_class_layouts,
    aura_direct_register_plain_layouts, aura_direct_release_value, aura_direct_retain_value,
    aura_direct_vec_push_in_place, boxed_value, list_view_of, register_plain_layout,
    share_direct_value, value_mut, value_ref, with_direct_task_runtime_scope,
    with_task_runtime_error_capture, DirectPlainLayoutEntry, ElementBox, OpaqueValue,
    UNREGISTERED_PLAIN_ACCESS,
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
        element_type: crate::runtime_value::SharedType::from(Type::named("int64")),
        elements: ListElements::from(values.iter().map(|value| int64(*value)).collect::<Vec<_>>()),
        str_view: Default::default(),
    })
}

fn text(value: &str) -> Value {
    Value::String(value.to_string().into())
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
        encode_descriptor(&Type::named(format!("{prefix}{class}")), &book_env(prefix))
            .expect("a layout")
            .into_boxed_str(),
    )
}

fn layout(prefix: &str, access: u32) -> &'static ClassLayout {
    register_class_layout(descriptor(prefix, "Book"), access).expect("the class is laid out")
}

fn meta(prefix: &str, name: &str, count: i64) -> Value {
    Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from(format!("{prefix}Meta")),
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
        class_name: crate::runtime_value::RuntimeName::from(class_name.to_string()),
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
    )
    .expect("a layout");
    let Err(plain) = register_plain_layout(&pair, Some(FIRST_CLASS_ACCESS)) else {
        panic!("a class code");
    };
    assert!(
        plain.contains("is not a plain layout access code"),
        "{plain}"
    );
}

#[test]
fn a_block_of_scalars_and_boxes_drops_by_releasing_its_boxes() {
    // `Book` holds boxes, a nested class, a tuple, and scalars: dropping
    // it only releases boxes.
    let book = layout("DropBoxes", FIRST_TEST_ACCESS + 0x200);
    assert!(book.drops_boxes_only);
    let fields = typed_book("DropBoxes", FIRST_TEST_ACCESS + 0x200);
    let title = fields
        .data
        .as_ptr()
        .wrapping_add(book.field("title").expect("a field").offset);
    // SAFETY: the slot holds the title's box, which the block owns.
    let boxed = unsafe { (*title.cast::<Option<ElementBox>>()).as_ref() }
        .map(ElementBox::as_ptr)
        .expect("a title box");
    // SAFETY: the box is live; this keeps it past the drop.
    unsafe { crate::native_runtime::retain_untracked_value(boxed) };
    drop(fields);
    // SAFETY: the reference taken above keeps the box live.
    let header = unsafe { &*boxed };
    assert_eq!(header.ref_count.load(Ordering::Relaxed), 1);
    assert_eq!(unsafe { value_ref(boxed) }, text("t"));
    // SAFETY: the last reference.
    unsafe { crate::native_runtime::release_untracked_value(boxed) };

    // A field with variants drops by its layout.
    let env = LayoutEnv {
        classes: [(
            "DropTagged".to_string(),
            ClassDefinition {
                type_params: Vec::new(),
                fields: vec![(
                    "maybe".to_string(),
                    Type::normalize_union(
                        vec![Type::named("str"), Type::Unit],
                        "main",
                        &Default::default(),
                    )
                    .expect("a valid union"),
                )],
            },
        )]
        .into(),
        enums: Default::default(),
    };
    let descriptor = encode_descriptor(&Type::named("DropTagged"), &env).expect("a layout");
    let tagged = register_class_layout(
        Box::leak(descriptor.into_boxed_str()),
        FIRST_TEST_ACCESS + 0x201,
    )
    .expect("the class is laid out");
    assert!(!tagged.drops_boxes_only);
    // With its field moved out, as generated code builds it.
    drop(TypedFields::new(tagged));
}

#[test]
fn a_new_typed_instance_has_the_shape_of_its_class_tag() {
    in_task(|| {
        let prefix = "Tagged";
        let layout = layout(prefix, FIRST_TEST_ACCESS + 0x210);
        let cell: &'static ClassLayoutCell = Box::leak(Box::default());
        cell.access.store(layout.access(), Ordering::Release);
        cell.layout
            .store(std::ptr::from_ref(layout).cast_mut(), Ordering::Release);
        let built = unsafe { aura_direct_instance_new_typed(cell) };
        // The tag generated code names the class with, through its cell.
        let descriptor = crate::native_runtime::canonical_runtime_type_name(&Type::named(format!(
            "{prefix}Book"
        )));
        let tag_cell: &'static std::sync::atomic::AtomicPtr<std::ffi::c_void> =
            Box::leak(Box::default());
        let tag = crate::native_runtime::celled_runtime_type_tag(tag_cell, descriptor.as_bytes());
        let header = unsafe { &*built };
        assert!(std::ptr::eq(
            header.shape_tag.load(Ordering::Relaxed),
            std::ptr::from_ref(tag)
        ));
        assert_eq!(
            unsafe { crate::native_runtime::explicit_runtime_type_name(built) },
            Some(descriptor)
        );
        unsafe { aura_direct_release_value(built) };
    });
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
        let descriptor = encode_descriptor(&ty, &env).expect("a layout");
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
    // A class code in the plain layout table is refused: classes register
    // through a table of their own.
    let cell_address = std::ptr::from_ref(cell) as usize;
    let text = descriptor;
    let refused = runtime_failure(move || {
        let entry = DirectPlainLayoutEntry {
            cell: cell_address as *const _,
            descriptor: text.as_ptr(),
            len: text.len(),
            access: u64::from(FIRST_TEST_ACCESS + 3),
        };
        unsafe { aura_direct_register_plain_layouts(&entry, 1) };
    });
    assert!(refused.contains("invalid plain layout"), "{refused}");
    unsafe { aura_direct_register_class_layouts(&entry, 1) };
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
        unsafe { aura_direct_register_class_layouts(&entry, 1) };
    });
    assert!(failure.contains("invalid class layout"), "{failure}");
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
    assert!(!copy.into_map().contains_key("title"));
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
    assert_eq!(
        list_view_of(&value),
        (FIRST_TEST_ACCESS + 30, data, 0, 0, 0)
    );
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
        // Every instance names its class with the layout's own text, which
        // lives as long as the program, so building one copies no name.
        let other = unsafe { aura_direct_instance_new_typed(cell) };
        let (Value::Instance(first), Value::Instance(second)) =
            (unsafe { value_ref(built) }, unsafe { value_ref(other) })
        else {
            panic!("instances");
        };
        assert_eq!(first.class_name.as_str(), layout.class_name());
        assert!(first.class_name.is_same(&second.class_name));
        unsafe { aura_direct_release_value(other) };
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
            element_type: crate::runtime_value::SharedType::from(Type::named(format!(
                "{prefix}Book"
            ))),
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
            element_type: crate::runtime_value::SharedType::from(Type::named(format!(
                "{prefix}Book"
            ))),
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
                .expect("a layout")
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
        )
        .expect("a layout");
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
        aura_direct_element_path_store(
            book,
            path.as_ptr(),
            path.len(),
            selectors.as_ptr(),
            1,
            boxed_value(int64(20)),
            0,
            0,
        );
        assert_eq!(field_of(book, "items"), ints(&[1, 20]));
        let path = "meta.count";
        aura_direct_element_path_store(
            book,
            path.as_ptr(),
            path.len(),
            [0i64; 0].as_ptr(),
            0,
            boxed_value(int64(30)),
            0,
            0,
        );
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

/// A cell that holds `layout`, as the program's registration fills one.
fn cell_of(layout: &'static ClassLayout) -> &'static ClassLayoutCell {
    let cell: &'static ClassLayoutCell = Box::leak(Box::default());
    cell.access.store(layout.access(), Ordering::Release);
    cell.layout
        .store(std::ptr::from_ref(layout).cast_mut(), Ordering::Release);
    cell
}

/// An empty `list[{prefix}Meta]` in a box.
fn meta_list(prefix: &str) -> *mut OpaqueValue {
    boxed_value(Value::Vec(VecValue {
        element_type: crate::runtime_value::SharedType::from(Type::named(format!("{prefix}Meta"))),
        elements: ListElements::new(),
        str_view: Default::default(),
    }))
}

/// Appends a new `Meta(name, count)` to `list` as generated code does for
/// `list.append(Meta(...))`: the block's words, with the name's box.
fn push_meta(list: *mut OpaqueValue, cell: &'static ClassLayoutCell, name: &str, count: i64) {
    let layout = unsafe { super::registered_class_layout(cell) }.expect("a registered layout");
    let mut words = vec![0u64; layout.stride_words()];
    let name_at = layout.field("name").expect("a name").offset / 8;
    let count_at = layout.field("count").expect("a count").offset / 8;
    words[name_at] = boxed_value(text(name)) as u64;
    words[count_at] = count as u64;
    unsafe { crate::native_runtime::aura_direct_vec_push_instance(list, cell, words.as_mut_ptr()) };
}

/// A copy of the elements of `list`, which shares their storage.
fn list_elements(list: *mut OpaqueValue) -> ListElements {
    match unsafe { value_ref(list) } {
        Value::Vec(vector) => vector.elements,
        other => panic!("a list, found {other:?}"),
    }
}

#[test]
fn classes_of_scalars_and_text_keep_their_instances_in_lists() {
    let prefix = "InList";
    let meta = register_class_layout(descriptor(prefix, "Meta"), FIRST_TEST_ACCESS + 80)
        .expect("the class is laid out");
    assert!(meta.keeps_in_list());
    assert_eq!(meta.stride_words(), 2);
    assert_eq!(meta.box_offsets().len(), 1);
    assert_eq!(
        meta.instance_list_access(),
        super::FIRST_INSTANCE_LIST_ACCESS + 80 + 0x1000_0000
    );
    // A list field lies in a box no list may keep in place.
    assert!(!layout(prefix, FIRST_TEST_ACCESS + 81).keeps_in_list());
}

#[test]
fn appended_instances_lie_in_the_list_and_read_as_instances() {
    in_task(|| {
        let prefix = "Lying";
        let meta = register_class_layout(descriptor(prefix, "Meta"), FIRST_TEST_ACCESS + 82)
            .expect("the class is laid out");
        let cell = cell_of(meta);
        let list = meta_list(prefix);
        push_meta(list, cell, "a", 1);
        push_meta(list, cell, "b", 2);
        push_meta(list, cell, "c", 3);
        let header = unsafe { &*list };
        assert_eq!(
            header.list_access.load(Ordering::Relaxed),
            meta.instance_list_access()
        );
        assert_eq!(header.list_len.get(), 3);
        let elements = list_elements(list);
        assert!(matches!(elements, ListElements::Instances(_)));
        assert_eq!(
            elements.form(),
            crate::list_elements::ListForm::Instances(meta.instance_list_access())
        );
        let expected = vec![
            meta_value(prefix, "a", 1),
            meta_value(prefix, "b", 2),
            meta_value(prefix, "c", 3),
        ];
        // Every read copies the element, which stays in the list.
        assert_eq!(elements.len(), 3);
        assert_eq!(elements.at(1).into_owned(), expected[1]);
        assert_eq!(elements.get(3), None);
        assert_eq!(elements.to_vec(), expected);
        assert_eq!(
            elements
                .iter()
                .rev()
                .map(Cow::into_owned)
                .collect::<Vec<_>>(),
            expected.iter().rev().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            elements.iter().nth(2).map(Cow::into_owned),
            Some(expected[2].clone())
        );
        assert_eq!(
            elements.iter().nth_back(2).map(Cow::into_owned),
            Some(expected[0].clone())
        );
        assert_eq!(elements.iter().len(), 3);
        assert!(elements.contains(&expected[2]));
        assert!(!elements.contains(&meta_value(prefix, "c", 4)));
        assert_eq!(elements, ListElements::from(expected.clone()));
        assert_eq!(elements, expected);
        assert_eq!(
            format!("{elements:?}"),
            format!("{:?}", ListElements::from(expected.clone()))
        );
        assert!(elements.value_refs().is_none());
        // A field of an element reads where it lies.
        assert_eq!(
            elements
                .instance_field(1, "name")
                .map(|field| field.map(Cow::into_owned)),
            Some(Some(text("b")))
        );
        assert_eq!(
            elements
                .instance_field(2, "count")
                .map(|field| field.map(Cow::into_owned)),
            Some(Some(int64(3)))
        );
        assert_eq!(elements.instance_field(3, "count"), Some(None));
        assert_eq!(elements.instance_field(0, "missing"), None);
        assert_eq!(
            ListElements::from(expected.clone()).instance_field(0, "name"),
            None
        );
        // So does an element path's field, and a whole element is a copy.
        for (path, selector, want) in [
            ("[i].name", -1i64, text("c")),
            ("[i].count", 1, int64(2)),
            ("[i]", 0, expected[0].clone()),
        ] {
            let loaded = aura_direct_element_path_load(
                list,
                path.as_ptr(),
                path.len(),
                [selector].as_ptr(),
                1,
                0,
                0,
            );
            assert_eq!(unsafe { value_ref(loaded) }, want);
            unsafe { aura_direct_release_value(loaded) };
        }
        assert!(elements.capacity() >= 3);
        // The copy shares the list's storage.
        assert!(elements.is_shared());
        assert_ne!(elements.as_ptr(), 0);
        // A slice copies its elements.
        let Value::Vec(vector) = (unsafe { value_ref(list) }) else {
            panic!("a list");
        };
        let slice = crate::runtime_value::slice_vec_owned(&vector, Some(1), None)
            .expect("the slice is in range");
        assert_eq!(slice.elements, expected[1..].to_vec());
        // A copy shares the elements until one of the two changes.
        let mut copy = elements.clone();
        copy.try_reserve(4).expect("room");
        assert!(!copy.is_shared());
        assert_ne!(copy.as_ptr(), elements.as_ptr());
        assert_eq!(copy, expected);
        let mut copy = elements.clone();
        copy.push(meta_value(prefix, "d", 4));
        assert!(matches!(copy, ListElements::Values(_)));
        assert_eq!(copy.len(), 4);
        assert_eq!(elements, expected);
        // Lending an element moves the list to boxes.
        let mut lent = elements.clone();
        assert_eq!(
            lent.lend(0).map(|element| element.value().clone()),
            Some(expected[0].clone())
        );
        assert!(matches!(lent, ListElements::Boxes(_)));
        let mut moved = elements.clone();
        assert_eq!(std::mem::take(&mut moved).into_vec(), expected);
        unsafe { aura_direct_release_value(list) };
    });
    // A field read past the end traps as an index past the end does.
    let failure = runtime_failure(|| {
        let meta = register_class_layout(descriptor("Lying", "Meta"), FIRST_TEST_ACCESS + 82)
            .expect("the class is laid out");
        let list = meta_list("Lying");
        push_meta(list, cell_of(meta), "a", 1);
        let path = "[i].count";
        aura_direct_element_path_load(list, path.as_ptr(), path.len(), [3i64].as_ptr(), 1, 0, 0);
    });
    assert!(failure.contains("out of bounds"), "{failure}");
}

#[test]
fn edits_of_kept_instances_take_the_value_form_first() {
    in_task(|| {
        let prefix = "Edited";
        let meta = register_class_layout(descriptor(prefix, "Meta"), FIRST_TEST_ACCESS + 84)
            .expect("the class is laid out");
        let cell = cell_of(meta);
        let list = meta_list(prefix);
        for (name, count) in [("x", 3), ("y", 1), ("z", 2)] {
            push_meta(list, cell, name, count);
        }
        let kept = list_elements(list);
        let values = ListElements::from(kept.to_vec());
        type Edit = fn(&mut ListElements) -> Value;
        let edits: Vec<(&str, Edit)> = vec![
            ("pop", |list| list.pop().unwrap_or(Value::Unit)),
            ("remove", |list| list.remove(1)),
            ("replace", |list| list.replace(0, Value::Bool(true))),
            ("insert", |list| {
                list.insert(1, Value::Unit);
                Value::Unit
            }),
            ("keep_every", |list| {
                list.keep_every(2);
                Value::Unit
            }),
            ("truncate", |list| {
                list.truncate(1);
                Value::Unit
            }),
            ("take_range", |list| {
                Value::Bool(list.take_range(0, 2).len() == 2)
            }),
            ("shuffle", |list| {
                list.shuffle(&mut crate::randomness::DeterministicRng::from_seed(7));
                Value::Unit
            }),
            ("get_mut", |list| {
                list.get_mut(0)
                    .map(|element| (*element).clone())
                    .unwrap_or(Value::Unit)
            }),
            ("append", |list| {
                let other = list.clone();
                list.append(other);
                Value::Unit
            }),
            ("insert_all", |list| {
                let other = list.clone();
                list.insert_all(1, other);
                Value::Unit
            }),
        ];
        for (name, edit) in edits {
            let mut left = kept.clone();
            let mut right = values.clone();
            assert_eq!(edit(&mut left), edit(&mut right), "{name}");
            assert_eq!(left, right, "{name}");
            assert!(!matches!(left, ListElements::Instances(_)), "{name}");
        }
        // A swap, a reversal and a sort by keys move the instances' words
        // and keep the form, in a list that shares them or not.
        type Move = fn(&mut ListElements);
        let moves: Vec<(&str, Move)> = vec![
            ("swap", |list| list.swap(0, 2)),
            ("swap back", |list| list.swap(2, 0)),
            ("swap itself", |list| list.swap(1, 1)),
            ("reverse", ListElements::reverse),
        ];
        for (name, edit) in moves {
            let mut left = kept.clone();
            let mut right = values.clone();
            edit(&mut left);
            edit(&mut right);
            assert_eq!(left, right, "{name}");
            assert!(matches!(left, ListElements::Instances(_)), "{name}");
            // Twice, the second time on storage the first one unshared.
            edit(&mut left);
            edit(&mut right);
            assert_eq!(left, right, "{name}");
        }
        assert_eq!(kept, values);
        let keys = ListElements::from(vec![int64(2), int64(0), int64(1)]);
        let mut left = kept.clone();
        let mut right = values.clone();
        crate::list_sort::sort_elements_by_keys(&mut left, &keys, false).expect("sorted");
        crate::list_sort::sort_elements_by_keys(&mut right, &keys, false).expect("sorted");
        assert_eq!(left, right);
        assert!(matches!(left, ListElements::Instances(_)));
        let swapped =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| kept.clone().swap(0, 3)))
                .expect_err("a swap past the end panics");
        assert!(swapped
            .downcast_ref::<String>()
            .is_some_and(|message| message.contains("out of range for length 3")),);
        // Sorting reports what sorting the values reports.
        let mut left = kept.clone();
        let mut right = values.clone();
        assert_eq!(
            crate::list_sort::sort_elements(&mut left, false).map_err(|error| error.message),
            crate::list_sort::sort_elements(&mut right, false).map_err(|error| error.message),
        );
        let keys = kept.clone();
        let mut left = values.clone();
        let mut right = values.clone();
        assert_eq!(
            crate::list_sort::sort_elements_by_keys(&mut left, &keys, false)
                .map_err(|error| error.message),
            crate::list_sort::sort_elements_by_keys(&mut right, &values, false)
                .map_err(|error| error.message),
        );
        // A clear drops the instances.
        let mut cleared = kept.clone();
        cleared.clear();
        assert!(cleared.is_empty());
        unsafe { aura_direct_release_value(list) };
    });
}

#[test]
fn other_lists_take_an_appended_instance_in_its_own_box() {
    in_task(|| {
        let prefix = "Elsewhere";
        let meta = register_class_layout(descriptor(prefix, "Meta"), FIRST_TEST_ACCESS + 86)
            .expect("the class is laid out");
        let cell = cell_of(meta);
        // A list that keeps other values takes a boxed instance.
        let list = meta_list(prefix);
        aura_direct_vec_push_in_place(list, boxed_value(meta_value(prefix, "a", 1)));
        push_meta(list, cell, "b", 2);
        assert!(!matches!(list_elements(list), ListElements::Instances(_)));
        assert_eq!(
            list_elements(list).to_vec(),
            vec![meta_value(prefix, "a", 1), meta_value(prefix, "b", 2)]
        );
        unsafe { aura_direct_release_value(list) };
        // So does a list that does not know its element type, a shared
        // one, and one that keeps another class in place.
        let untyped = boxed_value(Value::Vec(VecValue {
            element_type: crate::runtime_value::SharedType::from(Type::named("Unknown")),
            elements: ListElements::new(),
            str_view: Default::default(),
        }));
        push_meta(untyped, cell, "c", 3);
        assert!(!matches!(
            list_elements(untyped),
            ListElements::Instances(_)
        ));
        assert_eq!(
            list_elements(untyped).to_vec(),
            vec![meta_value(prefix, "c", 3)]
        );
        unsafe { aura_direct_release_value(untyped) };
        let shared = meta_list(prefix);
        unsafe { share_direct_value(shared) };
        push_meta(shared, cell, "d", 4);
        assert!(!matches!(list_elements(shared), ListElements::Instances(_)));
        unsafe { aura_direct_release_value(shared) };
        let other =
            register_class_layout(descriptor("ElsewhereOther", "Meta"), FIRST_TEST_ACCESS + 87)
                .expect("the class is laid out");
        let mixed = meta_list(prefix);
        push_meta(mixed, cell_of(other), "e", 5);
        push_meta(mixed, cell, "f", 6);
        assert!(!matches!(list_elements(mixed), ListElements::Instances(_)));
        assert_eq!(list_elements(mixed).len(), 2);
        unsafe { aura_direct_release_value(mixed) };
        // A class whose fields a list cannot keep takes the boxed path too.
        let book = layout(prefix, FIRST_TEST_ACCESS + 88);
        let books = meta_list(prefix);
        let mut words = vec![0u64; book.stride_words()];
        for offset in book.box_offsets() {
            words[offset / 8] = boxed_value(text("t")) as u64;
        }
        unsafe {
            crate::native_runtime::aura_direct_vec_push_instance(
                books,
                cell_of(book),
                words.as_mut_ptr(),
            )
        };
        assert!(!matches!(list_elements(books), ListElements::Instances(_)));
        assert_eq!(list_elements(books).len(), 1);
        unsafe { aura_direct_release_value(books) };
    });
    let failure = runtime_failure(|| {
        let meta = register_class_layout(descriptor("NullField", "Meta"), FIRST_TEST_ACCESS + 89)
            .expect("the class is laid out");
        let mut words = vec![0u64; meta.stride_words()];
        unsafe {
            crate::native_runtime::aura_direct_vec_push_instance(
                meta_list("NullField"),
                cell_of(meta),
                words.as_mut_ptr(),
            )
        };
    });
    assert!(failure.contains("null field value"), "{failure}");
}

fn meta_value(prefix: &str, name: &str, count: i64) -> Value {
    meta(prefix, name, count)
}

#[test]
fn shared_typed_fields_support_overlapping_and_concurrent_reads() {
    let typed = typed_book("SharedRead", FIRST_TEST_ACCESS + 90);
    let first = typed.get("title");
    let second = typed.get("title");
    assert_eq!(first, Some(Cow::Borrowed(&text("t"))));
    assert_eq!(second, first);
    assert!(typed.contains_key("title"));
    assert_eq!(typed.len(), 7);

    std::thread::scope(|scope| {
        for _ in 0..4 {
            let shared = &typed;
            scope.spawn(move || {
                for _ in 0..200 {
                    let a = shared.get("title");
                    let b = shared.get("items");
                    assert_eq!(a.as_deref(), Some(&text("t")));
                    assert_eq!(b.as_deref(), Some(&ints(&[1, 2])));
                    assert!(shared.contains_key("title"));
                    assert_eq!(shared.iter().count(), 7);
                }
            });
        }
    });

    typed.refresh_views();
    assert_eq!(typed.get("title").as_deref(), Some(&text("t")));
}
