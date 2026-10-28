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
    Value::String(value.to_string())
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
        let shared = unsafe { aura_direct_enum_new_typed(cell, 0) };
        unsafe { share_direct_value(shared) };
        assert_eq!(unsafe { &*shared }.list_access.load(Ordering::Relaxed), 0);
        unsafe { aura_direct_release_value(shared) };
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
