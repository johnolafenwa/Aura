//! The runtime side of plain records (typed-layout step 2c): registering
//! layouts, lists that keep records in place, and the helpers generated code
//! calls to box, unbox, read, append, and replace records.

use aura_core::layout::{encode_descriptor, ClassDefinition, EnumDefinition, LayoutEnv};
use aura_core::types::Type;

use super::{
    aura_direct_box_plain, aura_direct_instance_read_plain, aura_direct_register_plain_layouts,
    aura_direct_unbox_plain, aura_direct_vec_empty, aura_direct_vec_push_plain,
    aura_direct_vec_read_plain, aura_direct_vec_set_plain, boxed_value, list_view_of,
    register_plain_layout, registered_plain_layout, value_mut, value_ref,
    with_direct_task_runtime_scope, with_task_runtime_error_capture, DirectPlainLayoutEntry,
    OpaqueValue, PlainLayout, PlainLayoutCell, UNREGISTERED_PLAIN_ACCESS,
};
use crate::list_elements::{ListElements, ListForm};
use crate::runtime_value::{
    run_lightweight_root_task, union_value, EnumVariantValue, InstanceValue, TupleValue, Value,
    VecValue,
};

/// The access codes these tests register under, far above the codes
/// layouts registered without one take and below the class codes.
const FIRST_TEST_ACCESS: u32 = 0x3000_0000;

fn record_env(name: &str) -> LayoutEnv {
    LayoutEnv {
        classes: [(
            name.to_string(),
            ClassDefinition {
                type_params: Vec::new(),
                fields: vec![
                    ("id".to_string(), Type::named("int32")),
                    ("weight".to_string(), Type::named("float64")),
                    ("on".to_string(), Type::named("bool")),
                ],
            },
        )]
        .into(),
        enums: Default::default(),
    }
}

fn record(name: &str, id: i64, weight: f64) -> Value {
    Value::Instance(InstanceValue {
        class_name: name.to_string(),
        fields: [
            (
                "id".to_string(),
                Value::Int(
                    crate::integer::IntegerValue::from_typed_signed(
                        i128::from(id),
                        crate::integer::IntegerKind::Int32,
                    )
                    .expect("the id fits int32"),
                ),
            ),
            ("weight".to_string(), Value::Float(weight)),
            ("on".to_string(), Value::Bool(id % 2 == 0)),
        ]
        .into(),
    })
}

/// A table entry for class `name`'s layout under `access`, with a cell and
/// a descriptor that live for the rest of the process.
fn entry(name: &str, access: u64) -> DirectPlainLayoutEntry {
    let descriptor: &'static str =
        Box::leak(encode_descriptor(&Type::named(name), &record_env(name)).into_boxed_str());
    DirectPlainLayoutEntry {
        cell: Box::leak(Box::new(PlainLayoutCell::new())),
        descriptor: descriptor.as_ptr(),
        len: descriptor.len(),
        access,
    }
}

/// Registers class `name` under `access` and returns its cell and layout.
fn registered(name: &str, access: u32) -> (*const PlainLayoutCell, &'static PlainLayout) {
    let entry = entry(name, u64::from(access));
    unsafe { aura_direct_register_plain_layouts(&entry, 1) };
    let layout = unsafe { registered_plain_layout(entry.cell) }.expect("the cell is filled");
    assert_eq!(layout.access(), access);
    (entry.cell, layout)
}

/// The layout bytes of `value`, in whole words.
fn encoded(layout: &PlainLayout, value: &Value) -> Vec<u64> {
    let mut words = vec![0u64; layout.stride_words()];
    unsafe { layout.write_checked(value, words.as_mut_ptr().cast()) }
        .expect("the value fits the layout");
    words
}

fn decoded(layout: &PlainLayout, words: &[u64]) -> Value {
    unsafe { layout.read(words.as_ptr().cast()) }
}

fn list_form(list: *mut OpaqueValue) -> u32 {
    let mut access = 0;
    unsafe {
        value_mut(list, |value| {
            access = list_view_of(value).0;
        })
    };
    access
}

/// `name | None`, with the index of `name` among its members.
fn optional(name: &str) -> (Type, usize) {
    let union = Type::normalize_union(
        vec![Type::named(name), Type::Unit],
        "main",
        &Default::default(),
    )
    .expect("the union is valid");
    let Type::Union(members) = &union else {
        panic!("two members make a union");
    };
    let index = members
        .members
        .iter()
        .position(|member| *member == Type::named(name))
        .expect("the record is a member");
    (union, index)
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

#[test]
fn registered_records_live_in_lists_and_cross_with_one_call() {
    let name = "PlainTestReadWrite";
    let (cell, layout) = registered(name, FIRST_TEST_ACCESS);
    // Registering again under the same code is a no-op.
    let (again, same) = registered(name, FIRST_TEST_ACCESS);
    assert!(std::ptr::eq(layout, same));
    assert_ne!(cell, again);

    let list = aura_direct_vec_empty();
    for id in 0..5 {
        let words = encoded(layout, &record(name, id, id as f64 / 2.0));
        unsafe { aura_direct_vec_push_plain(list, words.as_ptr().cast(), cell) };
    }
    assert_eq!(list_form(list), FIRST_TEST_ACCESS);

    let mut words = vec![0u64; layout.stride_words()];
    unsafe { aura_direct_vec_read_plain(list, -2, 0, 0, words.as_mut_ptr().cast(), cell) };
    assert_eq!(decoded(layout, &words), record(name, 3, 1.5));

    let replacement = encoded(layout, &record(name, 40, 9.0));
    unsafe { aura_direct_vec_set_plain(list, 1, 0, 0, replacement.as_ptr().cast(), cell) };
    unsafe { aura_direct_vec_read_plain(list, 1, 0, 0, words.as_mut_ptr().cast(), cell) };
    assert_eq!(decoded(layout, &words), record(name, 40, 9.0));
    let Value::Vec(vector) = (unsafe { value_ref(list) }) else {
        panic!("a list stays a list");
    };
    assert_eq!(vector.elements.len(), 5);
    assert_eq!(
        vector.elements.get(1).unwrap().into_owned(),
        record(name, 40, 9.0)
    );

    // A list that keeps the records as values reads them the same way.
    let mut elements = ListElements::from(vec![record(name, 7, 0.25)]);
    elements.values_mut();
    assert_eq!(elements.form(), ListForm::Values);
    let values = boxed_value(Value::Vec(VecValue {
        element_type: Type::named(name),
        elements,
        str_view: Default::default(),
    }));
    unsafe { aura_direct_vec_read_plain(values, 0, 0, 0, words.as_mut_ptr().cast(), cell) };
    assert_eq!(decoded(layout, &words), record(name, 7, 0.25));
    // And stores one as a value.
    unsafe { aura_direct_vec_set_plain(values, 0, 0, 0, replacement.as_ptr().cast(), cell) };
    let Value::Vec(vector) = (unsafe { value_ref(values) }) else {
        panic!("a list stays a list");
    };
    assert_eq!(vector.elements.form(), ListForm::Values);
    assert_eq!(
        vector.elements.get(0).unwrap().into_owned(),
        record(name, 40, 9.0)
    );

    // A box is made from the bytes and read back into them.
    let boxed = unsafe { aura_direct_box_plain(replacement.as_ptr().cast(), cell) };
    assert_eq!(unsafe { value_ref(boxed) }, record(name, 40, 9.0));
    let mut unboxed = vec![0u64; layout.stride_words()];
    unsafe { aura_direct_unbox_plain(boxed, unboxed.as_mut_ptr().cast(), cell) };
    assert_eq!(decoded(layout, &unboxed), record(name, 40, 9.0));

    // A record field is read from an instance, an enum payload, or a
    // union's payload in place.
    let holder = boxed_value(Value::Instance(InstanceValue {
        class_name: "Holder".to_string(),
        fields: [("best".to_string(), record(name, 2, 4.0))].into(),
    }));
    let field = b"best";
    unsafe {
        aura_direct_instance_read_plain(
            holder,
            field.as_ptr(),
            field.len(),
            words.as_mut_ptr().cast(),
            cell,
        )
    };
    assert_eq!(decoded(layout, &words), record(name, 2, 4.0));
    let variant = boxed_value(Value::EnumVariant(EnumVariantValue {
        enum_name: "Slot".to_string().into(),
        variant_name: "Full".to_string().into(),
        payloads: vec![record(name, 6, 1.0)],
    }));
    let projection = b"__variant_payload_Full_0";
    unsafe {
        aura_direct_instance_read_plain(
            variant,
            projection.as_ptr(),
            projection.len(),
            words.as_mut_ptr().cast(),
            cell,
        )
    };
    assert_eq!(decoded(layout, &words), record(name, 6, 1.0));
    let (union_type, index) = optional(name);
    let union = boxed_value(union_value(union_type, index, record(name, 8, 2.0)));
    let projection = format!("__union_payload_{index}");
    unsafe {
        aura_direct_instance_read_plain(
            union,
            projection.as_ptr(),
            projection.len(),
            words.as_mut_ptr().cast(),
            cell,
        )
    };
    assert_eq!(decoded(layout, &words), record(name, 8, 2.0));
}

#[test]
fn registration_refuses_tables_codes_and_descriptors_it_cannot_honor() {
    // An empty table registers nothing and reads nothing.
    unsafe { aura_direct_register_plain_layouts(std::ptr::null(), 0) };
    let name = "PlainTestCodes";
    registered(name, FIRST_TEST_ACCESS + 10);
    let other = entry("PlainTestOtherCodes", 0);
    let other_descriptor = (other.descriptor as usize, other.len);
    let cases: Vec<(&str, Box<dyn FnOnce() + Send>)> = vec![
        (
            "direct runtime received a null plain layout table",
            Box::new(|| unsafe { aura_direct_register_plain_layouts(std::ptr::null(), 1) }),
        ),
        (
            "direct runtime received a null layout descriptor",
            Box::new(|| {
                let mut table = entry("PlainTestNull", u64::from(FIRST_TEST_ACCESS + 11));
                table.descriptor = std::ptr::null();
                unsafe { aura_direct_register_plain_layouts(&table, 1) }
            }),
        ),
        (
            "invalid plain layout: direct runtime received a null layout cell",
            Box::new(|| {
                let mut table = entry("PlainTestNullCell", u64::from(FIRST_TEST_ACCESS + 12));
                table.cell = std::ptr::null();
                unsafe { aura_direct_register_plain_layouts(&table, 1) }
            }),
        ),
        (
            "invalid plain layout: the layout is registered under access code 805306378, not 805306387",
            Box::new(|| {
                let table = entry("PlainTestCodes", u64::from(FIRST_TEST_ACCESS + 19));
                unsafe { aura_direct_register_plain_layouts(&table, 1) }
            }),
        ),
        (
            "invalid plain layout: access code 805306378 names another layout",
            Box::new(move || {
                let table = DirectPlainLayoutEntry {
                    cell: Box::leak(Box::new(PlainLayoutCell::new())),
                    descriptor: other_descriptor.0 as *const u8,
                    len: other_descriptor.1,
                    access: u64::from(FIRST_TEST_ACCESS + 10),
                };
                unsafe { aura_direct_register_plain_layouts(&table, 1) }
            }),
        ),
        (
            "invalid plain layout: 7 is not a plain layout access code",
            Box::new(|| {
                let table = entry("PlainTestLowCode", 7);
                unsafe { aura_direct_register_plain_layouts(&table, 1) }
            }),
        ),
        (
            "invalid plain layout: a layout descriptor is not UTF-8",
            Box::new(|| {
                let bytes: &'static [u8] = &[0xff, 0xfe];
                let table = DirectPlainLayoutEntry {
                    cell: Box::leak(Box::new(PlainLayoutCell::new())),
                    descriptor: bytes.as_ptr(),
                    len: bytes.len(),
                    access: u64::from(FIRST_TEST_ACCESS + 13),
                };
                unsafe { aura_direct_register_plain_layouts(&table, 1) }
            }),
        ),
    ];
    // `str` holds a handle, so it has no plain layout.
    assert_eq!(
        register_plain_layout(
            &encode_descriptor(&Type::named("str"), &LayoutEnv::default()),
            Some(FIRST_TEST_ACCESS + 14)
        )
        .err(),
        Some("`str` has no plain layout".to_string())
    );
    assert!(register_plain_layout("not a descriptor", None).is_err());
    for (expected, work) in cases {
        assert_eq!(runtime_failure(work), expected);
    }
}

#[test]
fn record_helpers_report_bad_cells_shapes_and_positions() {
    let name = "PlainTestErrors";
    let (cell, layout) = registered(name, FIRST_TEST_ACCESS + 20);
    let cell = cell as usize;
    let stride = layout.stride_words();
    let unregistered = Box::leak(Box::new(PlainLayoutCell::new())) as *const _ as usize;
    let list_of = move |ids: &[i64]| {
        boxed_value(Value::Vec(VecValue {
            element_type: Type::named(name),
            elements: ids
                .iter()
                .map(|id| record(name, *id, 0.0))
                .collect::<Vec<_>>()
                .into(),
            str_view: Default::default(),
        })) as usize
    };
    let cases: Vec<(&str, Box<dyn FnOnce() + Send>)> = vec![
        (
            "direct runtime received a null layout cell",
            Box::new(move || unsafe {
                aura_direct_box_plain(vec![0u64; stride].as_ptr().cast(), std::ptr::null());
            }),
        ),
        (
            "a plain layout was used before it was registered",
            Box::new(move || unsafe {
                aura_direct_box_plain(
                    vec![0u64; stride].as_ptr().cast(),
                    unregistered as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "a `str` value does not fit the layout of `PlainTestErrors`",
            Box::new(move || unsafe {
                let text = boxed_value(Value::String("x".to_string()));
                aura_direct_unbox_plain(
                    text,
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "list index `3` is out of bounds for length `2`",
            Box::new(move || unsafe {
                aura_direct_vec_read_plain(
                    list_of(&[1, 2]) as *mut OpaqueValue,
                    3,
                    0,
                    0,
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "list index `-3` is out of bounds for length `2`",
            Box::new(move || unsafe {
                let words = vec![0u64; stride];
                aura_direct_vec_set_plain(
                    list_of(&[1, 2]) as *mut OpaqueValue,
                    -3,
                    0,
                    0,
                    words.as_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "a `str` value does not fit the layout of `PlainTestErrors`",
            Box::new(move || unsafe {
                let list = boxed_value(Value::Vec(VecValue {
                    element_type: Type::named(name),
                    elements: vec![Value::String("x".to_string())].into(),
                    str_view: Default::default(),
                }));
                aura_direct_vec_read_plain(
                    list,
                    0,
                    0,
                    0,
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "a `str` value does not fit the layout of `PlainTestErrors`",
            Box::new(move || unsafe {
                let holder = boxed_value(Value::Instance(InstanceValue {
                    class_name: "Holder".to_string(),
                    fields: [("best".to_string(), Value::String("x".to_string()))].into(),
                }));
                let field = b"best";
                aura_direct_instance_read_plain(
                    holder,
                    field.as_ptr(),
                    field.len(),
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "class `Holder` has no field `missing`",
            Box::new(move || unsafe {
                let holder = boxed_value(Value::Instance(InstanceValue {
                    class_name: "Holder".to_string(),
                    fields: Default::default(),
                }));
                let field = b"missing";
                aura_direct_instance_read_plain(
                    holder,
                    field.as_ptr(),
                    field.len(),
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "cannot access field `best` on non-instance `integer`",
            Box::new(move || unsafe {
                let field = b"best";
                aura_direct_instance_read_plain(
                    boxed_value(Value::Int(crate::integer::IntegerValue::from_i64(1))),
                    field.as_ptr(),
                    field.len(),
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "enum payload projection does not select the active variant",
            Box::new(move || unsafe {
                let variant = boxed_value(Value::EnumVariant(EnumVariantValue {
                    enum_name: "Slot".to_string().into(),
                    variant_name: "Empty".to_string().into(),
                    payloads: Vec::new(),
                }));
                let projection = b"__variant_payload_Full_0";
                aura_direct_instance_read_plain(
                    variant,
                    projection.as_ptr(),
                    projection.len(),
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
        (
            "union payload projection does not select the active member",
            Box::new(move || unsafe {
                let (union_type, index) = optional(name);
                let union = boxed_value(union_value(union_type, 1 - index, Value::Unit));
                let projection = format!("__union_payload_{index}");
                aura_direct_instance_read_plain(
                    union,
                    projection.as_ptr(),
                    projection.len(),
                    vec![0u64; stride].as_mut_ptr().cast(),
                    cell as *const PlainLayoutCell,
                );
            }),
        ),
    ];
    for (expected, work) in cases {
        assert_eq!(runtime_failure(work), expected);
    }
    assert_eq!(UNREGISTERED_PLAIN_ACCESS, u32::MAX);
}

#[test]
fn enums_of_scalars_lay_out_plain_and_cells_start_empty() {
    // An enum whose payloads are scalars needs no drop code, so it has a
    // plain layout of a tag and the payload.
    let env = LayoutEnv {
        classes: Default::default(),
        enums: [(
            "PlainTestSlot".to_string(),
            EnumDefinition {
                type_params: Vec::new(),
                variants: vec![
                    ("Empty".to_string(), Vec::new()),
                    ("Full".to_string(), vec![Type::named("int64")]),
                ],
            },
        )]
        .into(),
    };
    let layout = register_plain_layout(
        &encode_descriptor(&Type::named("PlainTestSlot"), &env),
        None,
    )
    .expect("the enum has a plain layout");
    for (variant, payloads) in [
        ("Empty", Vec::new()),
        (
            "Full",
            vec![Value::Int(crate::integer::IntegerValue::from_i64(5))],
        ),
    ] {
        let value = Value::EnumVariant(EnumVariantValue {
            enum_name: "PlainTestSlot".to_string().into(),
            variant_name: variant.to_string().into(),
            payloads,
        });
        assert_eq!(decoded(layout, &encoded(layout, &value)), value);
    }

    let cell = PlainLayoutCell::default();
    assert_eq!(
        unsafe { registered_plain_layout(&cell) }.err(),
        Some("a plain layout was used before it was registered".to_string())
    );
}

#[test]
fn tuples_with_small_integers_and_unions_lay_out_plain() {
    let (union_type, int_index) = {
        let union = Type::normalize_union(
            vec![Type::named("int32"), Type::Unit],
            "main",
            &Default::default(),
        )
        .expect("the union is valid");
        let Type::Union(members) = &union else {
            panic!("two members make a union");
        };
        let index = members
            .members
            .iter()
            .position(|member| *member == Type::named("int32"))
            .expect("int32 is a member");
        (union, index)
    };
    let element_types = vec![
        Type::named("int8"),
        Type::named("uint16"),
        Type::Unit,
        union_type.clone(),
    ];
    let tuple_type = Type::Tuple(element_types.clone());
    let layout =
        register_plain_layout(&encode_descriptor(&tuple_type, &LayoutEnv::default()), None)
            .expect("the tuple has a plain layout");
    let int = |value: i128, kind| {
        Value::Int(
            crate::integer::IntegerValue::from_signed(value)
                .with_runtime_kind(kind)
                .expect("the value fits"),
        )
    };
    let tuple = |small: i128, member: usize, payload: Value| {
        Value::Tuple(TupleValue {
            element_types: element_types.clone(),
            elements: vec![
                int(small, crate::integer::IntegerKind::Int8),
                int(300 + small, crate::integer::IntegerKind::Uint16),
                Value::Unit,
                union_value(union_type.clone(), member, payload),
            ],
        })
    };
    let values = vec![
        tuple(-5, int_index, int(7, crate::integer::IntegerKind::Int32)),
        tuple(6, 1 - int_index, Value::Unit),
    ];
    for value in &values {
        assert_eq!(decoded(layout, &encoded(layout, value)), *value);
    }
    // A registered tuple layout is found from a tuple value, so a list of
    // such tuples keeps them in place.
    let list = ListElements::from(values.clone());
    assert_eq!(list.form(), ListForm::Typed(layout.access()));
    assert_eq!(list.to_vec(), values);
}
