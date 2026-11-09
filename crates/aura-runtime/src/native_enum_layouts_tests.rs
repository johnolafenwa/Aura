//! Enum variants in their typed layouts (typed-layout step 2f): the
//! layouts, the payloads of one variant, and the helpers generated code and
//! the rest of the runtime reach them through.

use std::borrow::Cow;
use std::sync::atomic::Ordering;

use aura_core::layout::{
    encode_descriptor, encode_layouts, EnumDefinition, LayoutBuilder, LayoutEnv,
};
use aura_core::types::Type;

use super::{
    celled_enum_layout, register_enum_layout, registered_enum_layout, EnumLayout, EnumLayoutCell,
    TypedPayloads, FIRST_ENUM_ACCESS,
};
use crate::collection_index::hash_value;
use crate::integer::{IntegerKind, IntegerValue};
use crate::native_runtime::{
    aura_direct_enum_new_typed, aura_direct_instance_put_field, aura_direct_register_enum_layouts,
    aura_direct_release_value, aura_direct_variant_matches, aura_direct_variant_payload,
    aura_direct_variant_payload_int64, aura_direct_variant_take_payload, boxed_value, list_view_of,
    share_direct_value, value_mut, value_ref, with_direct_task_runtime_scope,
    with_task_runtime_error_capture, DirectPlainLayoutEntry, ElementBox, UNREGISTERED_PLAIN_ACCESS,
};
use crate::runtime_value::{
    run_lightweight_root_task, EnumVariantValue, RuntimeName, Value, VariantPayloads,
};

/// The enum codes these tests register under, far above the codes the
/// code generator gives enums.
const FIRST_TEST_ACCESS: u32 = FIRST_ENUM_ACCESS + 0x1000_0000;

fn int64(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn int32(value: i64) -> Value {
    Value::Int(
        IntegerValue::from_typed_signed(i128::from(value), IntegerKind::Int32)
            .expect("the value fits int32"),
    )
}

fn text(value: &str) -> Value {
    Value::String(value.to_string().into())
}

fn scalar(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, _) if matches!(name.as_str(), "int32" | "int64" | "float64"))
}

/// `Shape` (with `prefix`): a variant with no payloads, one with a scalar,
/// one with a string and a narrow scalar, and one that holds two shapes.
fn shape_env(prefix: &str) -> LayoutEnv {
    let shape = format!("{prefix}Shape");
    LayoutEnv {
        classes: Default::default(),
        enums: [(
            shape.clone(),
            EnumDefinition {
                type_params: Vec::new(),
                variants: vec![
                    ("Point".to_string(), Vec::new()),
                    ("Circle".to_string(), vec![Type::named("float64")]),
                    (
                        "Named".to_string(),
                        vec![Type::named("str"), Type::named("int32")],
                    ),
                    (
                        "Pair".to_string(),
                        vec![Type::named(&shape), Type::named(&shape)],
                    ),
                ],
            },
        )]
        .into(),
    }
}

fn descriptor(prefix: &str) -> &'static str {
    let env = shape_env(prefix);
    let mut builder = LayoutBuilder::new(&env);
    let root = builder
        .boxed_enum_layout(&Type::named(&format!("{prefix}Shape")), scalar)
        .expect("a layout")
        .expect("an enum");
    Box::leak(encode_layouts(builder.table(), root).into_boxed_str())
}

fn layout(prefix: &str, access: u32) -> &'static EnumLayout {
    register_enum_layout(descriptor(prefix), access).expect("the enum is laid out")
}

fn variant(prefix: &str, name: &str, payloads: Vec<Value>) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: RuntimeName::from(format!("{prefix}Shape")),
        variant_name: RuntimeName::from(name),
        payloads: payloads.into(),
    })
}

fn typed(prefix: &str, access: u32, name: &str, payloads: Vec<Value>) -> Value {
    let layout = layout(prefix, access);
    Value::EnumVariant(EnumVariantValue {
        enum_name: layout.enum_name().clone(),
        variant_name: RuntimeName::from(name),
        payloads: VariantPayloads::Typed(
            TypedPayloads::from_values(layout, name, payloads).expect("the payloads fit"),
        ),
    })
}

fn typed_payloads(value: &Value) -> &TypedPayloads {
    let Value::EnumVariant(variant) = value else {
        panic!("a variant");
    };
    variant.payloads.typed().expect("typed payloads")
}

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
fn enums_register_once_under_one_code() {
    let first = layout("RegisterOnce", FIRST_TEST_ACCESS);
    assert_eq!(first.access(), FIRST_TEST_ACCESS);
    assert_eq!(first.enum_name().as_str(), "RegisterOnceShape");
    assert_eq!(first.variant_index("Named"), Some(2));
    assert_eq!(first.variant_index("Square"), None);
    assert_eq!(first.variant_name(3).map(RuntimeName::as_str), Some("Pair"));
    assert!(first.variant_name(4).is_none());
    assert!(!first.runtime_type().is_empty());
    assert!(format!("{first:?}").contains("EnumLayout"));
    assert!(std::ptr::eq(
        first,
        register_enum_layout(descriptor("RegisterOnce"), FIRST_TEST_ACCESS).unwrap()
    ));
    // Another code for the same layout, a code another layout holds, and a
    // code outside the enum range are refused.
    let error = register_enum_layout(descriptor("RegisterOnce"), FIRST_TEST_ACCESS + 1)
        .expect_err("one code per layout");
    assert!(error.contains("registered under access code"), "{error}");
    let error = register_enum_layout(descriptor("RegisterOther"), FIRST_TEST_ACCESS)
        .expect_err("one layout per code");
    assert!(error.contains("names another layout"), "{error}");
    for code in [FIRST_ENUM_ACCESS - 1, UNREGISTERED_PLAIN_ACCESS] {
        let error =
            register_enum_layout(descriptor("RegisterOther"), code).expect_err("not an enum code");
        assert!(error.contains("not an enum layout access code"), "{error}");
    }
}

#[test]
fn enum_layouts_refuse_other_descriptors() {
    // A plain value and an enum held inline with its recursive edges on the
    // heap are not enums in a box.
    let int = encode_descriptor(&Type::named("int64"), &LayoutEnv::default()).unwrap();
    let error = register_enum_layout(&int, FIRST_TEST_ACCESS + 10).expect_err("not an enum");
    assert!(error.contains("not an enum laid out in a box"), "{error}");
    let inline = encode_descriptor(
        &Type::named("RefuseInlineShape"),
        &shape_env("RefuseInline"),
    )
    .unwrap();
    let error = register_enum_layout(&inline, FIRST_TEST_ACCESS + 11).expect_err("a heap block");
    assert!(error.contains("holds a heap block"), "{error}");
    let generic_env = LayoutEnv {
        classes: Default::default(),
        enums: [(
            "RefuseMaybe".to_string(),
            EnumDefinition {
                type_params: vec!["T".to_string()],
                variants: vec![("Some".to_string(), vec![Type::named("T")])],
            },
        )]
        .into(),
    };
    let mut builder = LayoutBuilder::new(&generic_env);
    let concrete = Type::Named("RefuseMaybe".to_string(), vec![Type::named("int64")]);
    let root = builder
        .boxed_enum_layout(&concrete, scalar)
        .unwrap()
        .expect("an enum");
    let generic = encode_layouts(builder.table(), root);
    let maybe = register_enum_layout(&generic, FIRST_TEST_ACCESS + 12).expect("a concrete enum");
    // A generic enum's variants carry the name tagging gives them, so they
    // compare equal to ones the runtime tags.
    assert!(maybe.enum_name().as_str().starts_with("RefuseMaybe"));
    assert_ne!(maybe.enum_name().as_str(), "RefuseMaybe");
    let error = register_enum_layout("aura-layout-v0:", FIRST_TEST_ACCESS + 13)
        .expect_err("an unknown version");
    assert!(error.contains("unknown version"), "{error}");
}

#[test]
fn typed_payloads_read_take_and_convert() {
    let prefix = "Payloads";
    let access = FIRST_TEST_ACCESS + 20;
    let circle = typed(prefix, access, "Circle", vec![Value::Float(1.5)]);
    let point = typed(prefix, access, "Point", Vec::new());
    let named = typed(prefix, access, "Named", vec![text("n"), int32(7)]);
    let pair = typed(prefix, access, "Pair", vec![circle.clone(), named.clone()]);
    let payloads = typed_payloads(&pair);
    assert_eq!(payloads.variant(), 3);
    assert_eq!(payloads.variant_name().as_str(), "Pair");
    assert_eq!(payloads.len(), 2);
    assert!(!payloads.is_empty());
    assert_ne!(payloads.data(), 0);
    assert!(std::ptr::eq(payloads.layout(), layout(prefix, access)));
    // A boxed payload is borrowed from its box, and an inline one read out.
    assert!(matches!(payloads.get(0), Some(Cow::Borrowed(value)) if *value == circle));
    assert!(payloads.payload_box(0).is_some());
    assert!(payloads.get(2).is_none());
    assert!(payloads.payload_box(2).is_none());
    let named_payloads = typed_payloads(&named);
    assert!(matches!(named_payloads.get(1), Some(Cow::Owned(value)) if value == int32(7)));
    assert!(named_payloads.payload_box(1).is_none());
    assert_eq!(
        named_payloads
            .iter()
            .map(Cow::into_owned)
            .collect::<Vec<_>>(),
        vec![text("n"), int32(7)]
    );
    assert_eq!(
        format!("{named_payloads:?}"),
        format!("{:?}", [text("n"), int32(7)])
    );
    // A variant with no payloads has no block.
    assert_eq!(typed_payloads(&point).data(), 0);
    assert!(typed_payloads(&point).is_empty());
    // Taking moves a payload out, leaving a unit or zeroed bytes.
    let mut taken = typed_payloads(&named).clone();
    assert_eq!(taken.take(0), Some(text("n")));
    assert_eq!(taken.take(0), Some(Value::Unit));
    assert_eq!(taken.get(0).map(Cow::into_owned), Some(Value::Unit));
    assert_eq!(taken.take(1), Some(int32(7)));
    assert_eq!(taken.take(1), Some(int32(0)));
    assert_eq!(taken.take(2), None);
    // A copy owns its own boxes, and both convert back to values.
    let copy = typed_payloads(&pair).clone();
    assert_ne!(
        copy.payload_box(0).map(ElementBox::as_ptr),
        typed_payloads(&pair).payload_box(0).map(ElementBox::as_ptr)
    );
    assert_eq!(copy.into_vec(), vec![circle.clone(), named.clone()]);
    assert_eq!(
        typed_payloads(&point).clone().into_vec(),
        Vec::<Value>::new()
    );
    // Payloads that do not fit come back.
    let layout = layout(prefix, access);
    let refused = TypedPayloads::from_values(layout, "Circle", vec![text("x")])
        .expect_err("a string does not fit a float");
    assert_eq!(refused, vec![text("x")]);
    assert!(TypedPayloads::from_values(layout, "Circle", Vec::new()).is_err());
    assert!(TypedPayloads::from_values(layout, "Square", Vec::new()).is_err());
}

#[test]
fn variant_payloads_read_the_same_in_both_forms() {
    let prefix = "Forms";
    let access = FIRST_TEST_ACCESS + 30;
    let values = variant(prefix, "Named", vec![text("n"), int32(7)]);
    let typed_value = typed(prefix, access, "Named", vec![text("n"), int32(7)]);
    assert_eq!(values, typed_value);
    assert_eq!(typed_value, values);
    assert_eq!(hash_value(&values), hash_value(&typed_value));
    assert_eq!(values.render(), typed_value.render());
    assert_eq!(format!("{values:?}"), format!("{typed_value:?}"));
    assert_eq!(typed_value.clone(), values);
    let point = variant(prefix, "Point", Vec::new());
    let nested = typed(
        prefix,
        access,
        "Pair",
        vec![typed_value.clone(), point.clone()],
    );
    let nested_values = variant(prefix, "Pair", vec![values.clone(), point.clone()]);
    assert_eq!(nested, nested_values);
    assert_eq!(hash_value(&nested), hash_value(&nested_values));
    assert_eq!(nested.render(), nested_values.render());
    let Value::EnumVariant(mut variant) = nested else {
        panic!("a variant");
    };
    assert_eq!(variant.payloads.len(), 2);
    assert!(!variant.payloads.is_empty());
    assert_eq!(
        variant.payloads.get(0).map(Cow::into_owned),
        Some(values.clone())
    );
    assert_eq!(
        variant
            .payloads
            .iter()
            .rev()
            .map(Cow::into_owned)
            .collect::<Vec<_>>(),
        vec![point.clone(), values.clone()]
    );
    assert_eq!(variant.payloads.take(1), Some(point));
    // Changing a payload turns the payloads into values first.
    variant.payloads[0] = int64(1);
    assert!(variant.payloads.typed().is_none());
    assert_eq!(variant.payloads[0], int64(1));
    assert_eq!(variant.payloads.values(), &[int64(1), Value::Unit]);
    let collected: VariantPayloads = vec![int64(1)].into_iter().collect();
    assert_eq!(collected, VariantPayloads::from([int64(1)]));
    assert_eq!(VariantPayloads::default().into_vec(), Vec::<Value>::new());
    let typed_value = typed(prefix, access, "Circle", vec![Value::Float(2.0)]);
    let Value::EnumVariant(variant) = typed_value else {
        panic!("a variant");
    };
    assert_eq!(variant.payloads.into_vec(), vec![Value::Float(2.0)]);
}

#[test]
fn typed_variants_copy_their_blocks() {
    let prefix = "Copies";
    let access = FIRST_TEST_ACCESS + 55;
    // A block of scalars copies as bytes into a block of its own.
    let circle = typed(prefix, access, "Circle", vec![Value::Float(2.5)]);
    let copy = circle.clone();
    assert_eq!(copy, circle);
    assert_ne!(typed_payloads(&copy).data(), typed_payloads(&circle).data());
    let Value::EnumVariant(circle_variant) = &circle else {
        panic!("a variant");
    };
    assert!(circle_variant.payloads.copy_flat(|_| false));
    // A payload box copies its value into a box of its own.
    let named = typed(prefix, access, "Named", vec![text("n"), int32(7)]);
    let copy = named.clone();
    assert_eq!(copy, named);
    let boxes = |value: &Value| typed_payloads(value).payload_box(0).map(ElementBox::as_ptr);
    assert_ne!(boxes(&copy), boxes(&named));
    let Value::EnumVariant(named_variant) = &named else {
        panic!("a variant");
    };
    assert!(named_variant
        .payloads
        .copy_flat(crate::runtime_value::is_clone_leaf));
    assert!(!named_variant.payloads.copy_flat(|_| false));
    let point = typed(prefix, access, "Point", Vec::new());
    assert_eq!(point.clone(), point);
}

#[test]
fn deep_typed_variants_copy_without_recursion() {
    let prefix = "Deep";
    let access = FIRST_TEST_ACCESS + 60;
    let depth = 20_000;
    let mut shape = typed(prefix, access, "Point", Vec::new());
    for _ in 0..depth {
        let circle = typed(prefix, access, "Circle", vec![Value::Float(1.5)]);
        shape = typed(prefix, access, "Pair", vec![circle, shape]);
    }
    // Copying the chain one level per call would overflow this stack.
    let (shape, copy) = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let copy = crate::runtime_value::try_clone_array_containing_value(&shape)
                .expect("the chain is copied");
            (shape, copy)
        })
        .expect("a thread")
        .join()
        .expect("the copy needs no deep stack");
    let (mut original, mut copied) = (&shape, &copy);
    let mut levels = 0;
    loop {
        let (left, right) = (typed_payloads(original), typed_payloads(copied));
        assert_eq!(left.variant_name(), right.variant_name());
        if left.is_empty() {
            break;
        }
        assert_eq!(
            right.get(0).map(Cow::into_owned),
            left.get(0).map(Cow::into_owned)
        );
        let (next, next_copy) = (
            left.payload_box(1).expect("the rest"),
            right.payload_box(1).expect("the copied rest"),
        );
        assert_ne!(next.as_ptr(), next_copy.as_ptr());
        (original, copied) = (next.value(), next_copy.value());
        levels += 1;
    }
    assert_eq!(levels, depth);
    // Dropping a chain this deep recurses, so the test leaves both.
    std::mem::forget((shape, copy));
}

#[test]
#[should_panic(expected = "keeps its payloads in its layout")]
fn typed_payloads_have_no_value_slice() {
    let typed_value = typed("NoSlice", FIRST_TEST_ACCESS + 35, "Point", Vec::new());
    let Value::EnumVariant(variant) = typed_value else {
        panic!("a variant");
    };
    let _ = variant.payloads.values();
}

#[test]
fn generated_code_builds_tests_and_reads_typed_variants() {
    in_task(|| {
        let prefix = "Built";
        let layout = layout(prefix, FIRST_TEST_ACCESS + 40);
        let cell: &'static EnumLayoutCell = Box::leak(Box::default());
        let registered =
            unsafe { celled_enum_layout(cell, descriptor(prefix).as_bytes(), layout.access()) }
                .expect("registered");
        assert!(std::ptr::eq(registered, layout));
        assert!(std::ptr::eq(
            unsafe { registered_enum_layout(cell) }.unwrap(),
            layout
        ));
        // A second registration reads the cell.
        assert!(std::ptr::eq(
            unsafe { celled_enum_layout(cell, b"", 0) }.unwrap(),
            layout
        ));
        let built = unsafe { aura_direct_enum_new_typed(cell, 2) };
        let header = unsafe { &*built };
        // The box shows the code, the variant, and the block.
        assert_eq!(header.list_access.load(Ordering::Relaxed), layout.access());
        assert_eq!(header.list_len.get(), 2);
        let block = header.list_elements.get();
        assert_ne!(block, 0);
        let name = boxed_value(text("n"));
        unsafe { aura_direct_instance_put_field(block as *mut Option<ElementBox>, name) };
        // A scalar payload lies at its offset.
        unsafe { *((block + 8) as *mut i32) = 7 };
        assert_eq!(
            unsafe { value_ref(built) },
            variant(prefix, "Named", vec![text("n"), int32(7)])
        );
        let matches = |variant: &[u8]| {
            aura_direct_variant_matches(
                built,
                b"BuiltShape".as_ptr(),
                10,
                variant.as_ptr(),
                variant.len(),
            )
        };
        assert_eq!(matches(b"Named"), 1);
        assert_eq!(matches(b"Point"), 0);
        assert_eq!(aura_direct_variant_payload_int64(built, 1), 7);
        let read = aura_direct_variant_payload(built, 0);
        assert_eq!(unsafe { value_ref(read) }, text("n"));
        unsafe { aura_direct_release_value(read) };
        let taken = aura_direct_variant_take_payload(built, 0);
        // The payload's box moves out as it is.
        assert_eq!(taken, name);
        assert_eq!(unsafe { value_ref(taken) }, text("n"));
        unsafe { aura_direct_release_value(taken) };
        // A payload box something else holds is copied out.
        let other = unsafe { aura_direct_enum_new_typed(cell, 2) };
        let other_block = unsafe { &*other }.list_elements.get();
        let held = boxed_value(text("h"));
        unsafe { aura_direct_instance_put_field(other_block as *mut Option<ElementBox>, held) };
        let read = unsafe { crate::native_runtime::aura_direct_retain_value(held) };
        let copied = aura_direct_variant_take_payload(other, 0);
        assert_ne!(copied, held);
        assert_eq!(unsafe { value_ref(copied) }, text("h"));
        assert_eq!(unsafe { value_ref(read) }, text("h"));
        unsafe { aura_direct_release_value(copied) };
        unsafe { aura_direct_release_value(read) };
        unsafe { aura_direct_release_value(other) };
        // A change through the runtime turns the payloads into values, and
        // the view follows.
        unsafe {
            value_mut(built, |value| {
                let Value::EnumVariant(variant) = value else {
                    panic!("a variant");
                };
                variant.payloads[0] = text("m");
            })
        };
        assert_eq!(header.list_access.load(Ordering::Relaxed), 0);
        unsafe { aura_direct_release_value(built) };
        let shared = unsafe { aura_direct_enum_new_typed(cell, 1) };
        unsafe { share_direct_value(shared) };
        assert_eq!(unsafe { &*shared }.list_access.load(Ordering::Relaxed), 0);
        unsafe { aura_direct_release_value(shared) };
        // Every build of a variant with no payloads is one frozen box,
        // which keeps its view and its type's tag as its shape.
        let point = unsafe { aura_direct_enum_new_typed(cell, 0) };
        assert_eq!(unsafe { aura_direct_enum_new_typed(cell, 0) }, point);
        let header = unsafe { &*point };
        assert!(header.is_frozen());
        assert_eq!(header.list_access.load(Ordering::Relaxed), layout.access());
        assert_eq!(header.shape_tag.load(Ordering::Relaxed), layout.shape_tag());
        // A payload holds the shared box as it is, and gives it back as it
        // is when it moves out.
        let pair = unsafe { aura_direct_enum_new_typed(cell, 3) };
        let pair_block = unsafe { &*pair }.list_elements.get();
        unsafe { aura_direct_instance_put_field(pair_block as *mut Option<ElementBox>, point) };
        assert_eq!(aura_direct_variant_take_payload(pair, 0), point);
        unsafe { aura_direct_release_value(pair) };
        unsafe { aura_direct_release_value(point) };
        assert!(header.is_frozen());
    });
    let failure = runtime_failure(|| {
        let cell: &'static EnumLayoutCell = Box::leak(Box::default());
        unsafe { aura_direct_enum_new_typed(cell, 0) };
    });
    assert!(failure.contains("before it was registered"), "{failure}");
    let failure = runtime_failure(|| {
        let layout = layout("BadIndex", FIRST_TEST_ACCESS + 45);
        let cell: &'static EnumLayoutCell = Box::leak(Box::default());
        unsafe { celled_enum_layout(cell, descriptor("BadIndex").as_bytes(), layout.access()) }
            .expect("registered");
        unsafe { aura_direct_enum_new_typed(cell, 9) };
    });
    assert!(failure.contains("invalid enum variant index"), "{failure}");
    let cell: &'static EnumLayoutCell = Box::leak(Box::default());
    let error = unsafe { celled_enum_layout(cell, &[0xff], FIRST_TEST_ACCESS + 46) }.unwrap_err();
    assert!(error.contains("not UTF-8"), "{error}");
    let error = unsafe { celled_enum_layout(std::ptr::null(), b"", 0) }.unwrap_err();
    assert!(error.contains("null layout cell"), "{error}");
    let error = unsafe { registered_enum_layout(std::ptr::null()) }.unwrap_err();
    assert!(error.contains("null layout cell"), "{error}");
}

#[test]
fn the_layout_table_registers_enum_cells() {
    let prefix = "Table";
    let descriptor = descriptor(prefix);
    let cell: &'static EnumLayoutCell = Box::leak(Box::default());
    let cell_address = std::ptr::from_ref(cell) as usize;
    in_task(move || {
        let entry = DirectPlainLayoutEntry {
            cell: cell_address as *const _,
            descriptor: descriptor.as_ptr(),
            len: descriptor.len(),
            access: u64::from(FIRST_TEST_ACCESS + 50),
        };
        unsafe { aura_direct_register_enum_layouts(&entry, 1) }
    });
    let layout = unsafe { registered_enum_layout(cell) }.expect("registered");
    assert_eq!(layout.access(), FIRST_TEST_ACCESS + 50);
    // A variant kept as values shows no view.
    assert_eq!(list_view_of(&variant(prefix, "Point", Vec::new())).0, 0);
    let typed_value = typed(
        prefix,
        FIRST_TEST_ACCESS + 50,
        "Circle",
        vec![Value::Float(1.0)],
    );
    let (access, block, index, _) = list_view_of(&typed_value);
    assert_eq!((access, index), (FIRST_TEST_ACCESS + 50, 1));
    assert_eq!(block, typed_payloads(&typed_value).data());
}

/// A dict of typed variants under `str` keys, as generated code builds one.
fn shadow_map(prefix: &str, access: u32) -> *mut crate::native_runtime::OpaqueValue {
    use crate::native_runtime::{aura_direct_map_empty, aura_direct_map_insert_in_place};
    let map = aura_direct_map_empty();
    for (key, value) in [
        (
            "ring",
            typed(prefix, access, "Named", vec![text("ring"), int32(3)]),
        ),
        (
            "dot",
            typed(prefix, access, "Circle", vec![Value::Float(1.5)]),
        ),
    ] {
        aura_direct_map_insert_in_place(map, boxed_value(text(key)), boxed_value(value));
    }
    map
}

#[test]
fn borrowing_entry_loans_read_the_dict_value_in_place() {
    use crate::native_runtime::{
        aura_direct_end_shadow, aura_direct_map_get_shadow, aura_direct_retain_value,
        SHADOW_REFERENCE,
    };
    run_lightweight_root_task(|| {
        with_direct_task_runtime_scope(|| {
            let access = FIRST_TEST_ACCESS + 65;
            let map = shadow_map("Shadow", access);
            let key = boxed_value(text("ring"));
            let read = |boxed| unsafe { value_ref(boxed) };
            let expected = typed("Shadow", access, "Named", vec![text("ring"), int32(3)]);

            // A confined dict lends its value: the box reads the entry's own
            // block, and it looks shared to every check.
            let shadow = aura_direct_map_get_shadow(map, key);
            let header = unsafe { &*shadow };
            assert_eq!(
                header.ref_count.load(Ordering::Acquire),
                SHADOW_REFERENCE + 1
            );
            let lent_block = typed_payloads(unsafe { &*header.value.get() }).data();
            let stored = unsafe { value_ref(map) };
            let Value::Map(stored) = &stored else {
                panic!("a dict");
            };
            let entry = stored.entries.value_of(&text("ring")).expect("the entry");
            assert_eq!(typed_payloads(entry).data(), lent_block);
            assert_eq!(read(shadow), expected);
            // Ending the loan leaves the dict's value as it was.
            unsafe { aura_direct_end_shadow(shadow) };
            assert_eq!(read(map), unsafe { value_ref(map) });
            let again = aura_direct_map_get_shadow(map, key);
            assert_eq!(read(again), expected);

            // A borrower that kept the box gets a copy of its own at the end.
            unsafe { aura_direct_retain_value(again) };
            unsafe { aura_direct_end_shadow(again) };
            let kept = unsafe { &*again };
            assert_eq!(kept.ref_count.load(Ordering::Acquire), 1);
            assert_ne!(
                typed_payloads(unsafe { &*kept.value.get() }).data(),
                lent_block
            );
            assert_eq!(read(again), expected);
            unsafe { aura_direct_release_value(again) };

            // A dict whose entries another dict shares hands out a copy.
            let shared = boxed_value(unsafe { value_ref(map) });
            let copy = aura_direct_map_get_shadow(map, key);
            assert_eq!(unsafe { &*copy }.ref_count.load(Ordering::Acquire), 1);
            assert_eq!(read(copy), expected);
            unsafe { aura_direct_end_shadow(copy) };
            // A missing key gives null, which ends as nothing.
            let missing = boxed_value(text("none"));
            let none = aura_direct_map_get_shadow(map, missing);
            assert!(none.is_null());
            unsafe { aura_direct_end_shadow(none) };
            for boxed in [shared, missing, key, map] {
                unsafe { aura_direct_release_value(boxed) };
            }
            Ok(Value::Unit)
        })
    })
    .expect("the loans run");
}

#[test]
fn slot_loans_walk_a_dicts_entries_without_keys() {
    use crate::native_runtime::{
        aura_direct_end_shadow, aura_direct_map_next_slot, aura_direct_map_shadow_at,
        SHADOW_REFERENCE,
    };
    run_lightweight_root_task(|| {
        with_direct_task_runtime_scope(|| {
            let access = FIRST_TEST_ACCESS + 66;
            let map = shadow_map("SlotShadow", access);
            // The copy of the dict that gives the values goes before the
            // walk, so the dict's entries are its own again.
            let expected = match unsafe { value_ref(map) } {
                Value::Map(stored) => stored
                    .entries
                    .iter()
                    .map(|(_, value)| value.clone())
                    .collect::<Vec<_>>(),
                _ => panic!("a dict"),
            };
            let mut seen = Vec::new();
            let mut slot = 0;
            loop {
                let next = aura_direct_map_next_slot(map, slot);
                if next < 0 {
                    break;
                }
                let shadow = aura_direct_map_shadow_at(map, next);
                let header = unsafe { &*shadow };
                assert_eq!(
                    header.ref_count.load(Ordering::Acquire),
                    SHADOW_REFERENCE + 1
                );
                seen.push(unsafe { value_ref(shadow) });
                unsafe { aura_direct_end_shadow(shadow) };
                slot = next + 1;
            }
            assert_eq!(seen, expected);
            assert_eq!(aura_direct_map_next_slot(map, -1), -1);
            // No entry in the slot, or a slot that is no slot, lends nothing.
            assert!(aura_direct_map_shadow_at(map, slot).is_null());
            assert!(aura_direct_map_shadow_at(map, -1).is_null());
            unsafe { aura_direct_release_value(map) };
            Ok(Value::Unit)
        })
    })
    .expect("the loans run");
}

#[test]
fn a_list_element_box_of_a_typed_variant_has_its_layouts_shape() {
    use crate::native_runtime::ElementBox;
    let access = FIRST_TEST_ACCESS + 67;
    let value = typed("ElementShape", access, "Circle", vec![Value::Float(2.0)]);
    let shape = typed_payloads(&value).layout().shape_tag();
    let element = ElementBox::new(value);
    let header = unsafe { &*element.as_ptr() };
    assert_eq!(header.shape_tag.load(Ordering::Relaxed), shape);
    // Any other value starts with no shape.
    let plain = ElementBox::new(Value::Float(2.0));
    assert!(unsafe { &*plain.as_ptr() }
        .shape_tag
        .load(Ordering::Relaxed)
        .is_null());
}

/// The `json.Value` layout generated code registers, which this thread's
/// parses then build their values in.
fn json_layout(access: u32) -> &'static EnumLayout {
    let json = Type::named("json.Value");
    let text_type = Type::named("str");
    let env = LayoutEnv {
        classes: Default::default(),
        enums: [(
            "json.Value".to_string(),
            EnumDefinition {
                type_params: Vec::new(),
                variants: vec![
                    ("Null".to_string(), Vec::new()),
                    ("Bool".to_string(), vec![Type::named("bool")]),
                    ("Int".to_string(), vec![Type::named("int64")]),
                    ("Float".to_string(), vec![Type::named("float64")]),
                    ("String".to_string(), vec![text_type.clone()]),
                    (
                        "Array".to_string(),
                        vec![Type::Named("list".to_string(), vec![json.clone()])],
                    ),
                    (
                        "Object".to_string(),
                        vec![Type::Named(
                            "dict".to_string(),
                            vec![text_type, json.clone()],
                        )],
                    ),
                ],
            },
        )]
        .into(),
    };
    let inline = |ty: &Type| matches!(ty, Type::Named(name, _) if matches!(name.as_str(), "int64" | "float64" | "bool"));
    let mut builder = LayoutBuilder::new(&env);
    let root = builder
        .boxed_enum_layout(&json, inline)
        .expect("a layout")
        .expect("an enum");
    let descriptor = Box::leak(encode_layouts(builder.table(), root).into_boxed_str());
    let layout = register_enum_layout(descriptor, access).expect("json.Value is laid out");
    assert!(std::ptr::eq(
        crate::native_runtime::json_value_layout().expect("registered"),
        layout
    ));
    layout
}

fn json_typed(layout: &'static EnumLayout, name: &str, payloads: Vec<Value>) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: layout.enum_name().clone(),
        variant_name: RuntimeName::from(name),
        payloads: VariantPayloads::Typed(
            TypedPayloads::from_values(layout, name, payloads).expect("the payloads fit"),
        ),
    })
}

fn parsed_json(source: &str) -> Value {
    let parsed = crate::runtime_value::json_parse_to_runtime(source).expect("the parse runs");
    let Value::EnumVariant(result) = parsed else {
        panic!("a Result");
    };
    assert_eq!(result.variant_name.as_str(), "Ok", "{source}");
    result.payloads.get(0).expect("the value").into_owned()
}

#[test]
fn json_parses_build_typed_variants_that_dump_and_read_in_place() {
    use crate::native_runtime::aura_direct_json_as_scalar;
    use crate::runtime_value::json_dumps_runtime;
    let layout = json_layout(FIRST_TEST_ACCESS + 68);
    // A parse keeps a document in a tape, but one whose object repeats a
    // key builds a tree, every variant in the layout, scalars in place.
    let source = r#"{"a":null,"b":true,"c":-3,"d":2.5,"e":"x","f":[1,[2,false]],"g":{"h":"y"}}"#;
    let taped = parsed_json(source);
    let Value::EnumVariant(variant) = &taped else {
        panic!("a variant");
    };
    assert!(variant.payloads.json().is_some());
    assert_eq!(json_dumps_runtime(&taped, None).expect("dumps"), source);
    let repeated = format!("{},\"a\":null}}", &source[..source.len() - 1]);
    let value = parsed_json(&repeated);
    assert!(std::ptr::eq(typed_payloads(&value).layout(), layout));
    assert_eq!(json_dumps_runtime(&value, None).expect("dumps"), source);
    assert_eq!(value, taped);
    // A source too long to parse on the task is kept in a tape too, and one
    // that repeats a key is not.
    let long = format!("[{}]", vec![r#"{"k":1}"#; 3000].join(","));
    let Value::EnumVariant(variant) = parsed_json(&long) else {
        panic!("a variant");
    };
    assert_eq!(variant.payloads.json().map(|node| node.span()), Some(9001));
    let long = format!("[{}]", vec![r#"{"k":1,"k":2}"#; 3000].join(","));
    let Value::EnumVariant(variant) = parsed_json(&long) else {
        panic!("a variant");
    };
    assert!(variant.payloads.json().is_none());
    let indented = json_dumps_runtime(&value, Some(1)).expect("dumps");
    assert!(indented.starts_with("{\n \"a\": null,\n"), "{indented}");
    for scalar in ["7", "-1.25", "true", "null", "\"s\"", "[]", "{}"] {
        let value = parsed_json(scalar);
        assert_eq!(json_dumps_runtime(&value, None).expect("dumps"), scalar);
    }

    // A float the text cannot spell takes the general path, which reports it.
    let nan = json_typed(layout, "Float", vec![Value::Float(f64::NAN)]);
    assert!(json_dumps_runtime(&nan, None).is_err());
    let nested = json_typed(
        layout,
        "Array",
        vec![Value::Vec(crate::runtime_value::VecValue {
            element_type: crate::runtime_value::SharedType::from(Type::named("json.Value")),
            elements: vec![
                json_typed(layout, "Bool", vec![Value::Bool(false)]),
                json_typed(layout, "Int", vec![int64(4)]),
                json_typed(layout, "Null", Vec::new()),
                nan.clone(),
            ]
            .into(),
            str_view: Default::default(),
        })],
    );
    assert!(json_dumps_runtime(&nested, None).is_err());

    // `json.as_int` and its kin read a scalar in place.
    let read = |value: &Value, kind: i64| {
        let boxed = boxed_value(value.clone());
        let mut bits = 0_i64;
        let found = aura_direct_json_as_scalar(boxed, kind, &mut bits);
        unsafe { aura_direct_release_value(boxed) };
        (found, bits)
    };
    let int = json_typed(layout, "Int", vec![int64(-9)]);
    let float = json_typed(layout, "Float", vec![Value::Float(0.5)]);
    let truth = json_typed(layout, "Bool", vec![Value::Bool(true)]);
    assert_eq!(read(&int, 0), (1, -9));
    assert_eq!(read(&float, 1), (1, 0.5_f64.to_bits() as i64));
    assert_eq!(read(&truth, 2), (1, 1));
    assert_eq!(read(&int, 1).0, 0);
    assert_eq!(read(&truth, 0).0, 0);
    // A value kept as payloads, or of another enum, takes the general path.
    let loose = Value::EnumVariant(EnumVariantValue {
        enum_name: RuntimeName::from("json.Value"),
        variant_name: RuntimeName::from("Int"),
        payloads: vec![int64(5)].into(),
    });
    assert_eq!(read(&loose, 0), (1, 5));
    // A value in a tape reads its node, and its box shows the node.
    for (source, kind, expected) in [
        ("-9", 0, (1, -9)),
        ("0.5", 1, (1, 0.5_f64.to_bits() as i64)),
        ("true", 2, (1, 1)),
        ("false", 2, (1, 0)),
        ("1", 1, (0, 0)),
        ("\"s\"", 0, (0, 0)),
    ] {
        let value = parsed_json(source);
        assert_eq!(read(&value, kind), expected, "{source}");
        let boxed = boxed_value(value.clone());
        let header = unsafe { &*boxed };
        let Value::EnumVariant(variant) = &value else {
            panic!("a variant");
        };
        let node = variant.payloads.json().expect("a tape value");
        assert_eq!(
            header
                .list_access
                .load(std::sync::atomic::Ordering::Relaxed),
            crate::native_runtime::JSON_TAPE_ACCESS
        );
        assert_eq!(header.list_elements.get(), node.address());
        unsafe { aura_direct_release_value(boxed) };
    }

    // Typed payloads made one at a time, by name or by index.
    let by_name = TypedPayloads::from_value(layout, "Int", int64(2)).expect("fits");
    assert_eq!(by_name.scalar_bits(0).map(|(_, bits)| bits), Some(2));
    let index = layout.variant_index("Float").expect("Float");
    let by_index = TypedPayloads::from_value_at(layout, index, Value::Float(1.0)).expect("fits");
    assert_eq!(
        by_index.scalar_bits(0).map(|(_, bits)| bits),
        Some(1.0_f64.to_bits() as i64)
    );
    let string = TypedPayloads::from_value(layout, "String", text("t")).expect("fits");
    assert_eq!(string.scalar_bits(0), None);
    assert!(TypedPayloads::from_value(layout, "Nope", int64(1)).is_err());
    assert!(TypedPayloads::from_value(layout, "Null", int64(1)).is_err());
    assert!(TypedPayloads::from_value(layout, "Int", text("no")).is_err());
    assert!(TypedPayloads::from_value_at(layout, 99, int64(1)).is_err());
    assert!(TypedPayloads::from_value_at(layout, 0, int64(1)).is_err());
    assert!(TypedPayloads::from_value_at(layout, index, text("no")).is_err());
}

#[test]
fn dict_literals_take_their_entry_types_from_the_first_entry() {
    use crate::native_runtime::{
        aura_direct_map_literal_entry, aura_direct_map_literal_str_entry,
        aura_direct_map_with_capacity,
    };
    let layout = json_layout(FIRST_TEST_ACCESS + 68);
    let types = |map| match unsafe { value_ref(map) } {
        Value::Map(map) => (
            Type::clone(&map.key_type).to_string(),
            Type::clone(&map.value_type).to_string(),
            map.entries.len(),
        ),
        _ => panic!("a dict"),
    };
    let object = aura_direct_map_with_capacity(3);
    for (key, distinct, value) in [
        ("a", 1, json_typed(layout, "Int", vec![int64(1)])),
        ("b", 1, json_typed(layout, "Null", Vec::new())),
        ("a", 0, json_typed(layout, "Bool", vec![Value::Bool(true)])),
    ] {
        aura_direct_map_literal_str_entry(
            object,
            key.as_ptr(),
            key.len(),
            boxed_value(value),
            distinct,
        );
    }
    assert_eq!(
        types(object),
        ("str".to_string(), "json.Value".to_string(), 2)
    );
    let names = aura_direct_map_with_capacity(-1);
    aura_direct_map_literal_str_entry(names, "k".as_ptr(), 1, boxed_value(text("v")), 0);
    assert_eq!(types(names), ("str".to_string(), "str".to_string(), 1));
    let numbers = aura_direct_map_with_capacity(2);
    for (key, value) in [(1, 10), (1, 11)] {
        aura_direct_map_literal_entry(numbers, boxed_value(int64(key)), boxed_value(int64(value)));
    }
    assert_eq!(
        types(numbers),
        ("int64".to_string(), "int64".to_string(), 1)
    );
    for map in [object, names, numbers] {
        unsafe { aura_direct_release_value(map) };
    }
    // A dict's shared entry type reads as the type it names.
    let shared = crate::runtime_value::SharedType::named("str");
    assert_eq!(AsRef::<Type>::as_ref(&shared), &Type::named("str"));
}

#[test]
fn an_entry_loan_of_a_value_that_is_no_dict_traps() {
    use crate::native_runtime::aura_direct_map_get_int64_shadow;
    let message = runtime_failure(|| {
        let number = boxed_value(int64(1));
        aura_direct_map_get_int64_shadow(number, 1);
    });
    assert!(message.contains("expected `dict`"), "{message}");
}

#[test]
fn int_keyed_entry_loans_and_literal_cells_lend_without_copies() {
    use crate::native_runtime::{
        aura_direct_end_shadow, aura_direct_map_get_int64_shadow, aura_direct_map_insert_in_place,
        aura_direct_string_literal_cell, SHADOW_REFERENCE,
    };
    run_lightweight_root_task(|| {
        with_direct_task_runtime_scope(|| {
            let access = FIRST_TEST_ACCESS + 69;
            let map = crate::native_runtime::aura_direct_map_empty();
            let stored = typed("IntShadow", access, "Circle", vec![Value::Float(3.5)]);
            aura_direct_map_insert_in_place(
                map,
                boxed_value(int64(7)),
                boxed_value(stored.clone()),
            );
            let shadow = aura_direct_map_get_int64_shadow(map, 7);
            assert_eq!(
                unsafe { &*shadow }.ref_count.load(Ordering::Acquire),
                SHADOW_REFERENCE + 1
            );
            assert_eq!(unsafe { value_ref(shadow) }, stored);
            unsafe { aura_direct_end_shadow(shadow) };
            assert!(aura_direct_map_get_int64_shadow(map, 8).is_null());
            unsafe { aura_direct_release_value(map) };

            // A literal's cell makes its frozen box once.
            let cell: &'static std::sync::atomic::AtomicUsize = Box::leak(Box::default());
            let literal = "cell";
            let first =
                unsafe { aura_direct_string_literal_cell(cell, literal.as_ptr(), literal.len()) };
            let second =
                unsafe { aura_direct_string_literal_cell(cell, literal.as_ptr(), literal.len()) };
            assert_eq!(first, second);
            assert_eq!(unsafe { value_ref(first) }, text("cell"));
            // No cell gives a box of its own.
            let plain = unsafe {
                aura_direct_string_literal_cell(std::ptr::null(), literal.as_ptr(), literal.len())
            };
            assert_ne!(plain, first);
            assert_eq!(unsafe { value_ref(plain) }, text("cell"));
            unsafe { aura_direct_release_value(plain) };
            Ok(Value::Unit)
        })
    })
    .expect("the loans run");
}

#[test]
fn a_variant_of_one_scalar_keeps_its_block_in_itself() {
    let access = FIRST_TEST_ACCESS + 70;
    let value = typed("Inline", access, "Circle", vec![Value::Float(2.5)]);
    let payloads = typed_payloads(&value);
    assert!(payloads.is_inline());
    // A variant with a box among its payloads keeps a heap block.
    let named = typed("Inline", access, "Named", vec![text("a"), int32(1)]);
    assert!(!typed_payloads(&named).is_inline());
    assert_eq!(
        payloads.scalar_bits(0).map(|(_, bits)| bits),
        Some(2.5_f64.to_bits() as i64)
    );

    // A box that takes the variant points its view at the block where the
    // variant now lies, inside the box.
    let boxed = boxed_value(value.clone());
    let header = unsafe { &*boxed };
    let held = unsafe { &*header.value.get() };
    assert_eq!(header.list_elements.get(), typed_payloads(held).data());
    assert_ne!(header.list_elements.get(), payloads.data());
    // A write through the box keeps the view on the block.
    unsafe { value_mut(boxed, |held| *held = value.clone()) };
    let held = unsafe { &*header.value.get() };
    assert_eq!(header.list_elements.get(), typed_payloads(held).data());
    assert_eq!(unsafe { value_ref(boxed) }, value);
    unsafe { aura_direct_release_value(boxed) };

    // Copies and moves keep the payload, and taking it leaves zero.
    let copy = value.clone();
    assert_eq!(copy, value);
    let Value::EnumVariant(mut variant) = copy else {
        panic!("a variant");
    };
    let VariantPayloads::Typed(typed_copy) = &mut variant.payloads else {
        panic!("typed payloads");
    };
    assert_eq!(typed_copy.take(0), Some(Value::Float(2.5)));
    assert_eq!(typed_copy.scalar_bits(0).map(|(_, bits)| bits), Some(0));
    let VariantPayloads::Typed(typed_copy) = std::mem::replace(
        &mut variant.payloads,
        VariantPayloads::from(Vec::<Value>::new()),
    ) else {
        panic!("typed payloads");
    };
    assert_eq!(typed_copy.into_vec(), vec![Value::Float(0.0)]);
}

#[test]
fn dict_lookups_read_a_parsed_objects_entries_in_its_tape() {
    use crate::native_runtime::{
        aura_direct_end_shadow, aura_direct_map_get, aura_direct_map_get_lent,
        aura_direct_map_get_payload, aura_direct_map_get_shadow, aura_direct_map_get_str,
        aura_direct_map_get_str_lent, aura_direct_map_get_str_payload,
    };
    run_lightweight_root_task(|| {
        with_direct_task_runtime_scope(|| {
            let tape =
                crate::json_tape::JsonTape::parse(r#"{"a": 1, "b": "x"}"#, 8, 100).expect("parses");
            let node = crate::json_tape::JsonNode::root(tape);
            let Some(fields @ Value::Map(_)) = node.payload() else {
                panic!("a dict");
            };
            let map = boxed_value(fields);
            let unrealized = || match unsafe { value_ref(map) } {
                Value::Map(map) => map.entries.unrealized_json().is_some(),
                _ => false,
            };
            let variant_of = |boxed: *mut crate::native_runtime::OpaqueValue| {
                assert!(!boxed.is_null());
                let value = unsafe { value_ref(boxed) };
                unsafe { aura_direct_release_value(boxed) };
                match value {
                    Value::EnumVariant(variant) => variant.variant_name.as_str().to_string(),
                    other => panic!("a variant, not {other:?}"),
                }
            };
            let key_a = boxed_value(text("a"));
            let key_z = boxed_value(text("z"));
            let key_int = boxed_value(int64(1));
            unsafe {
                assert_eq!(variant_of(aura_direct_map_get_lent(map, key_a)), "Int");
                assert!(aura_direct_map_get_lent(map, key_z).is_null());
                assert_eq!(
                    variant_of(aura_direct_map_get_str_lent(map, "b".as_ptr(), 1)),
                    "String"
                );
                assert!(aura_direct_map_get_str_lent(map, "q".as_ptr(), 1).is_null());
                let shadow = aura_direct_map_get_shadow(map, key_a);
                assert!(!shadow.is_null());
                aura_direct_end_shadow(shadow);
                assert!(aura_direct_map_get_shadow(map, key_z).is_null());
                assert_eq!(variant_of(aura_direct_map_get(map, key_a)), "Found");
                assert_eq!(variant_of(aura_direct_map_get(map, key_z)), "Missing");
                assert_eq!(
                    variant_of(aura_direct_map_get_str(map, "b".as_ptr(), 1)),
                    "Found"
                );
                assert_eq!(variant_of(aura_direct_map_get_payload(map, key_a)), "Int");
                assert_eq!(
                    variant_of(aura_direct_map_get_str_payload(map, "b".as_ptr(), 1)),
                    "String"
                );
            }
            // Every lookup so far read the tape; a key that is not a `str`
            // looks the entries up as values.
            assert!(unrealized());
            assert!(unsafe { aura_direct_map_get_lent(map, key_int) }.is_null());
            assert!(!unrealized());
            // A realized confined dict lends the entry itself.
            assert!(!unsafe { aura_direct_map_get_lent(map, key_a) }.is_null());
            for boxed in [key_a, key_z, key_int, map] {
                unsafe { aura_direct_release_value(boxed) };
            }
            Ok(Value::Unit)
        })
    })
    .expect("the lookups run");
}

#[test]
fn scalar_entry_loans_read_a_parsed_objects_nodes_where_they_lie() {
    use crate::native_runtime::{aura_direct_json_as_scalar, aura_direct_map_get_scalar_lent};
    run_lightweight_root_task(|| {
        with_direct_task_runtime_scope(|| {
            let tape =
                crate::json_tape::JsonTape::parse(r#"{"a": 7, "b": "x", "c": true}"#, 8, 100)
                    .expect("parses");
            let node = crate::json_tape::JsonNode::root(tape);
            let fields = node.payload().expect("a dict");
            // The value's own box and the dict over its tape lend the same
            // nodes.
            let doc = boxed_value(node.into_value());
            let map = boxed_value(fields);
            let [key_a, key_b, key_c, key_z] =
                ["a", "b", "c", "z"].map(|key| boxed_value(text(key)));
            let key_int = boxed_value(int64(1));
            let scalar = |word, kind| {
                let mut out = 0;
                let held = aura_direct_json_as_scalar(word, kind, &mut out);
                (held, out)
            };
            for object in [doc, map] {
                let lent = aura_direct_map_get_scalar_lent(object, key_a);
                assert_eq!(lent as usize & 3, 2, "a node's address");
                assert_eq!(scalar(lent, 0), (1, 7));
                assert_eq!(scalar(lent, 1), (0, 0));
                assert_eq!(scalar(lent, 2), (0, 0));
                let flag = aura_direct_map_get_scalar_lent(object, key_c);
                assert_eq!(scalar(flag, 2), (1, 1));
                let other = aura_direct_map_get_scalar_lent(object, key_b);
                assert_eq!(scalar(other, 0), (0, 0));
                assert!(aura_direct_map_get_scalar_lent(object, key_z).is_null());
            }
            // A key that is not a `str` looks the entries up as values, and
            // the dict then lends the value it keeps.
            assert!(aura_direct_map_get_scalar_lent(map, key_int).is_null());
            let lent = aura_direct_map_get_scalar_lent(map, key_a);
            assert_eq!(lent as usize & 3, 1, "a value's address");
            assert_eq!(scalar(lent, 0), (1, 7));

            // A value another thread can reach hands out a box instead.
            unsafe { share_direct_value(doc) };
            let copy = aura_direct_map_get_scalar_lent(doc, key_a);
            assert_eq!(copy as usize & 3, 0, "a box");
            assert_eq!(scalar(copy, 0), (1, 7));
            unsafe { aura_direct_release_value(copy) };
            assert!(aura_direct_map_get_scalar_lent(doc, key_z).is_null());
            for boxed in [key_a, key_b, key_c, key_z, key_int, map, doc] {
                unsafe { aura_direct_release_value(boxed) };
            }
            Ok(Value::Unit)
        })
    })
    .expect("the lookups run");
}

#[test]
fn lookups_read_a_parsed_objects_own_box_as_its_dict() {
    use crate::native_runtime::{
        aura_direct_end_shadow, aura_direct_map_get_payload, aura_direct_map_get_shadow,
    };
    run_lightweight_root_task(|| {
        with_direct_task_runtime_scope(|| {
            let tape = crate::json_tape::JsonTape::parse(r#"{"a": [1], "b": "x"}"#, 8, 100)
                .expect("parses");
            let doc = boxed_value(crate::json_tape::JsonNode::root(tape).into_value());
            let key_a = boxed_value(text("a"));
            let key_z = boxed_value(text("z"));
            let variant_of = |boxed: *mut crate::native_runtime::OpaqueValue| match unsafe {
                value_ref(boxed)
            } {
                Value::EnumVariant(variant) => variant.variant_name.as_str().to_string(),
                other => panic!("a variant, not {other:?}"),
            };
            // A borrowing loan of an entry in a tape takes a box of its own,
            // which its end releases.
            let shadow = aura_direct_map_get_shadow(doc, key_a);
            assert_eq!(variant_of(shadow), "Array");
            assert_eq!(unsafe { &*shadow }.ref_count.load(Ordering::Acquire), 1);
            unsafe { aura_direct_end_shadow(shadow) };
            assert!(aura_direct_map_get_shadow(doc, key_z).is_null());
            let copy = aura_direct_map_get_payload(doc, key_a);
            assert_eq!(variant_of(copy), "Array");
            unsafe { aura_direct_release_value(copy) };
            assert!(aura_direct_map_get_payload(doc, key_z).is_null());
            for boxed in [key_a, key_z, doc] {
                unsafe { aura_direct_release_value(boxed) };
            }
            Ok(Value::Unit)
        })
    })
    .expect("the lookups run");
}

#[test]
fn a_dict_lends_a_parsed_document_it_keeps_to_a_borrowing_loan() {
    use crate::native_runtime::{
        aura_direct_end_shadow, aura_direct_map_empty, aura_direct_map_get_int64_shadow,
        aura_direct_map_insert_in_place, SHADOW_REFERENCE,
    };
    run_lightweight_root_task(|| {
        with_direct_task_runtime_scope(|| {
            let tape = crate::json_tape::JsonTape::parse(r#"{"a": 1}"#, 8, 100).expect("parses");
            let doc = crate::json_tape::JsonNode::root(tape).into_value();
            let docs = aura_direct_map_empty();
            aura_direct_map_insert_in_place(docs, boxed_value(int64(4)), boxed_value(doc.clone()));
            // The box reads the entry's own reference to the tape, and it
            // looks shared to every check.
            let shadow = aura_direct_map_get_int64_shadow(docs, 4);
            assert_eq!(
                unsafe { &*shadow }.ref_count.load(Ordering::Acquire),
                SHADOW_REFERENCE + 1
            );
            assert_eq!(unsafe { value_ref(shadow) }, doc);
            // Ending the loan leaves the dict's document as it was.
            unsafe { aura_direct_end_shadow(shadow) };
            let again = aura_direct_map_get_int64_shadow(docs, 4);
            assert_eq!(unsafe { value_ref(again) }, doc);
            unsafe { aura_direct_end_shadow(again) };
            unsafe { aura_direct_release_value(docs) };
            Ok(Value::Unit)
        })
    })
    .expect("the loans run");
}
