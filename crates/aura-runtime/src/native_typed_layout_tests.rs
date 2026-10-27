use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::collections::BTreeMap;

use aura_core::integer::{IntegerKind, IntegerValue};
use aura_core::layout::{encode_descriptor, ClassDefinition, EnumDefinition, LayoutEnv};
use aura_core::types::Type;

use super::super::{canonical_runtime_type_name, explicit_runtime_type_tag, OpaqueValue};
use super::TypedLayout;
use crate::list_elements::ListElements;
use crate::runtime_value::{
    union_value, EnumVariantValue, InstanceValue, TupleValue, Value, VecValue,
    DIRECT_RUNTIME_TYPE_FIELD, DIRECT_RUNTIME_TYPE_SEPARATOR,
};

/// Zeroed memory for one value of a layout, freed when the test drops it.
struct Slot {
    ptr: *mut u8,
    layout: Layout,
}

impl Slot {
    fn new(typed: &TypedLayout) -> Self {
        let layout = Layout::from_size_align(typed.size().max(1), typed.align()).unwrap();
        // SAFETY: the size is at least one byte.
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(!ptr.is_null());
        Self { ptr, layout }
    }

    fn bytes(&self, typed: &TypedLayout) -> Vec<u8> {
        // SAFETY: the slot holds the layout's bytes.
        unsafe { std::slice::from_raw_parts(self.ptr, typed.size()).to_vec() }
    }

    fn word(&self, offset: usize) -> *mut OpaqueValue {
        // SAFETY: the test reads a pointer the layout keeps at `offset`.
        unsafe { self.ptr.add(offset).cast::<*mut OpaqueValue>().read() }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with this layout.
        unsafe { dealloc(self.ptr, self.layout) };
    }
}

fn typed(ty: &Type, env: &LayoutEnv) -> TypedLayout {
    TypedLayout::from_descriptor(&encode_descriptor(ty, env).expect("a layout")).expect("a layout")
}

fn write(typed: &TypedLayout, value: Value) -> Slot {
    let slot = Slot::new(typed);
    // SAFETY: the slot fits the layout.
    unsafe { typed.write(value, slot.ptr) }.expect("the value fits");
    slot
}

/// Writes `value`, reads it back twice, and takes it out.
fn assert_round_trip(typed: &TypedLayout, value: Value) {
    let slot = write(typed, value.clone());
    // SAFETY: the slot holds a value of the layout until the take.
    unsafe {
        assert_eq!(typed.read(slot.ptr), value);
        assert_eq!(typed.read(slot.ptr), value);
        assert_eq!(typed.take(slot.ptr), value);
    }
}

fn int(value: i128, kind: IntegerKind) -> Value {
    Value::Int(if kind.is_signed() {
        IntegerValue::from_typed_signed(value, kind).unwrap()
    } else {
        IntegerValue::from_typed_unsigned(value as u128, kind).unwrap()
    })
}

fn int64(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn string(text: &str) -> Value {
    Value::String(text.to_string())
}

fn instance(class: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Instance(InstanceValue {
        class_name: class.to_string(),
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    })
}

fn variant(enum_name: &str, name: &str, payloads: Vec<Value>) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: enum_name.into(),
        variant_name: name.into(),
        payloads: payloads.into(),
    })
}

fn tuple(element_types: Vec<Type>, elements: Vec<Value>) -> Value {
    Value::Tuple(TupleValue {
        element_types,
        elements,
    })
}

fn int64_list(values: &[i64]) -> Value {
    Value::Vec(VecValue::new(
        Type::named("int64"),
        values
            .iter()
            .map(|value| int64(*value))
            .collect::<ListElements>(),
    ))
}

fn list_storage_shared(value: &Value) -> bool {
    match value {
        Value::Vec(vector) => vector.elements.is_shared(),
        other => panic!("expected a list, found {other:?}"),
    }
}

fn union(members: Vec<Type>) -> Type {
    Type::normalize_union(members, "main", &BTreeMap::new()).expect("a valid union")
}

fn member_index(union_type: &Type, member: &Type) -> usize {
    let Type::Union(union) = union_type else {
        panic!("expected a union");
    };
    union
        .members
        .iter()
        .position(|candidate| candidate == member)
        .expect("a member")
}

fn class(type_params: &[&str], fields: &[(&str, Type)]) -> ClassDefinition {
    ClassDefinition {
        type_params: type_params.iter().map(|param| param.to_string()).collect(),
        fields: fields
            .iter()
            .map(|(name, ty)| (name.to_string(), ty.clone()))
            .collect(),
    }
}

fn enumeration(type_params: &[&str], variants: &[(&str, Vec<Type>)]) -> EnumDefinition {
    EnumDefinition {
        type_params: type_params.iter().map(|param| param.to_string()).collect(),
        variants: variants
            .iter()
            .map(|(name, payloads)| (name.to_string(), payloads.clone()))
            .collect(),
    }
}

#[test]
fn scalars_round_trip_in_their_width() {
    let env = LayoutEnv::default();
    let cases: Vec<(&str, Value, Vec<u8>)> = vec![
        ("int8", int(-5, IntegerKind::Int8), vec![0xfb]),
        ("uint8", int(200, IntegerKind::Uint8), vec![200]),
        ("int16", int(-2, IntegerKind::Int16), vec![0xfe, 0xff]),
        ("uint16", int(0x1234, IntegerKind::Uint16), vec![0x34, 0x12]),
        ("int32", int(-1, IntegerKind::Int32), vec![0xff; 4]),
        (
            "uint32",
            int(0x0102_0304, IntegerKind::Uint32),
            vec![4, 3, 2, 1],
        ),
        ("int64", int64(i64::MIN), {
            let mut bytes = vec![0; 7];
            bytes.push(0x80);
            bytes
        }),
        (
            "uint64",
            int(u64::MAX.into(), IntegerKind::Uint64),
            vec![0xff; 8],
        ),
        ("int128", int(i128::MIN, IntegerKind::Int128), {
            let mut bytes = vec![0; 15];
            bytes.push(0x80);
            bytes
        }),
        ("uint128", int(1, IntegerKind::Uint128), {
            let mut bytes = vec![1];
            bytes.extend([0; 15]);
            bytes
        }),
        ("intsize", int(-1, IntegerKind::IntSize), vec![0xff; 8]),
        ("uintsize", int(7, IntegerKind::UintSize), {
            let mut bytes = vec![7];
            bytes.extend([0; 7]);
            bytes
        }),
        ("bool", Value::Bool(true), vec![1]),
        ("float32", Value::Float(1.5), 1.5f32.to_le_bytes().to_vec()),
        (
            "float64",
            Value::Float(-0.25),
            (-0.25f64).to_le_bytes().to_vec(),
        ),
        (
            "Duration",
            Value::Duration(-1_500),
            (-1_500i128).to_le_bytes().to_vec(),
        ),
    ];
    for (name, value, bytes) in cases {
        let typed = typed(&Type::named(name), &env);
        let slot = write(&typed, value.clone());
        assert_eq!(slot.bytes(&typed), bytes, "{name}");
        assert_round_trip(&typed, value);
    }
    let unit = typed(&Type::Unit, &env);
    assert_eq!(unit.size(), 0);
    assert_round_trip(&unit, Value::Unit);
}

#[test]
fn values_that_do_not_fit_are_refused_before_anything_is_written() {
    let env = LayoutEnv::default();
    let int8 = typed(&Type::named("int8"), &env);
    let slot = Slot::new(&int8);
    // SAFETY: the slot fits the layout.
    let error = unsafe { int8.write(int64(300), slot.ptr) }.unwrap_err();
    assert_eq!(error, "integer value `300` does not fit in `int8`");
    let uint8 = typed(&Type::named("uint8"), &env);
    // SAFETY: as above.
    assert!(unsafe { uint8.write(int64(-1), slot.ptr) }.is_err());
    let flag = typed(&Type::named("bool"), &env);
    // SAFETY: as above.
    let error = unsafe { flag.write(int64(1), slot.ptr) }.unwrap_err();
    assert!(
        error.contains("does not fit the layout of `bool`"),
        "{error}"
    );

    // A tuple whose second element does not fit leaves the first unwritten.
    let pair = typed(
        &Type::Tuple(vec![Type::named("str"), Type::named("uint8")]),
        &env,
    );
    let slot = Slot::new(&pair);
    let value = tuple(
        vec![Type::named("str"), Type::named("uint8")],
        vec![string("kept"), int64(-3)],
    );
    // SAFETY: as above.
    assert!(unsafe { pair.write(value, slot.ptr) }.is_err());
    assert!(slot.bytes(&pair).iter().all(|byte| *byte == 0));
}

#[test]
fn classes_and_tuples_place_fields_at_their_offsets() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Point".to_string(),
        class(
            &[],
            &[
                ("x", Type::named("float64")),
                ("y", Type::named("int32")),
                ("visible", Type::named("bool")),
            ],
        ),
    );
    let point = typed(&Type::named("Point"), &env);
    assert_eq!((point.size(), point.align()), (16, 8));
    let value = instance(
        "Point",
        vec![
            ("x", Value::Float(2.0)),
            ("y", int(-7, IntegerKind::Int32)),
            ("visible", Value::Bool(true)),
        ],
    );
    let slot = write(&point, value.clone());
    let bytes = slot.bytes(&point);
    assert_eq!(&bytes[0..8], &2.0f64.to_le_bytes());
    assert_eq!(&bytes[8..12], &(-7i32).to_le_bytes());
    assert_eq!(bytes[12], 1);
    assert_round_trip(&point, value);

    let element_types = vec![Type::named("int32"), Type::named("float64")];
    let pair = typed(&Type::Tuple(element_types.clone()), &env);
    assert_round_trip(
        &pair,
        tuple(
            element_types,
            vec![int(3, IntegerKind::Int32), Value::Float(0.5)],
        ),
    );
}

#[test]
fn handles_own_boxes_that_reads_copy_and_takes_move() {
    let env = LayoutEnv::default();
    let element_types = vec![
        Type::named("str"),
        Type::Named("list".to_string(), vec![Type::named("int64")]),
    ];
    let pair = typed(&Type::Tuple(element_types.clone()), &env);
    let list = int64_list(&[1, 2, 3]);
    let value = tuple(element_types.clone(), vec![string("name"), list.clone()]);
    assert!(list_storage_shared(&list));

    // The handle carries its static type, as a box generated code makes does.
    let slot = write(&pair, value.clone());
    // SAFETY: the slot holds a pointer to a live box at offset 0.
    let name = unsafe { explicit_runtime_type_tag(slot.word(0)) };
    assert_eq!(
        name.as_deref(),
        Some(canonical_runtime_type_name(&Type::named("str")).as_str())
    );

    // SAFETY: the slots hold values of the layout until each take or drop.
    unsafe {
        assert_eq!(pair.read(slot.ptr), value);

        // A copy owns boxes of its own, so dropping the original leaves it.
        let copy = Slot::new(&pair);
        pair.clone_into(slot.ptr, copy.ptr);
        assert_ne!(copy.word(0), slot.word(0));
        pair.drop_in_place(slot.ptr);
        assert!(list_storage_shared(&list));
        assert_eq!(pair.take(copy.ptr), value);
    }
    // Both boxes are gone, so only `list` and `value` hold the storage.
    drop(value);
    assert!(!list_storage_shared(&list));

    // Taking frees the box and hands its contents over.
    let slot = write(
        &pair,
        tuple(element_types, vec![string("name"), list.clone()]),
    );
    // SAFETY: as above.
    let taken = unsafe { pair.take(slot.ptr) };
    let Value::Tuple(taken) = taken else {
        panic!("expected a tuple");
    };
    assert!(list_storage_shared(&list));
    drop(taken);
    assert!(!list_storage_shared(&list));
}

#[test]
fn erased_handles_carry_no_type() {
    let env = LayoutEnv::default();
    let erased = typed(&Type::TypeParam("T".to_string()), &env);
    let slot = write(&erased, int64(4));
    // SAFETY: the slot holds a pointer to a live box.
    unsafe {
        assert_eq!(explicit_runtime_type_tag(slot.word(0)), None);
        erased.drop_in_place(slot.ptr);
    }
}

#[test]
fn enums_write_the_tag_and_the_variant_payloads() {
    let mut env = LayoutEnv::default();
    env.enums.insert(
        "Shape".to_string(),
        enumeration(
            &[],
            &[
                ("Circle", vec![Type::named("float64")]),
                ("Rect", vec![Type::named("float32"), Type::named("float32")]),
                ("Empty", vec![]),
            ],
        ),
    );
    let shape = typed(&Type::named("Shape"), &env);
    assert_eq!((shape.size(), shape.align()), (16, 8));
    let rect = variant("Shape", "Rect", vec![Value::Float(1.0), Value::Float(2.0)]);
    let slot = write(&shape, rect.clone());
    let bytes = slot.bytes(&shape);
    assert_eq!(bytes[0], 1);
    assert_eq!(&bytes[8..12], &1.0f32.to_le_bytes());
    assert_eq!(&bytes[12..16], &2.0f32.to_le_bytes());
    for value in [
        variant("Shape", "Circle", vec![Value::Float(3.0)]),
        rect,
        variant("Shape", "Empty", vec![]),
    ] {
        assert_round_trip(&shape, value);
    }
}

#[test]
fn unions_tag_the_member_index() {
    let env = LayoutEnv::default();
    let union_type = union(vec![Type::named("int64"), Type::named("str")]);
    let typed = typed(&union_type, &env);
    for (member, payload) in [
        (Type::named("int64"), int64(9)),
        (Type::named("str"), string("nine")),
    ] {
        let index = member_index(&union_type, &member);
        let value = union_value(union_type.clone(), index, payload);
        let slot = write(&typed, value.clone());
        assert_eq!(usize::from(slot.bytes(&typed)[0]), index);
        assert_round_trip(&typed, value);
    }
}

#[test]
fn recursive_enums_keep_the_rest_in_heap_blocks() {
    let mut env = LayoutEnv::default();
    env.enums.insert(
        "IntList".to_string(),
        enumeration(
            &[],
            &[
                ("Cons", vec![Type::named("int64"), Type::named("IntList")]),
                ("Nil", vec![]),
            ],
        ),
    );
    let list_type = typed(&Type::named("IntList"), &env);
    // A tag, then the head and one pointer.
    assert_eq!(list_type.size(), 24);
    let value = [3, 2, 1]
        .into_iter()
        .fold(variant("IntList", "Nil", vec![]), |rest, head| {
            variant("IntList", "Cons", vec![int64(head), rest])
        });
    assert_round_trip(&list_type, value.clone());

    let slot = write(&list_type, value.clone());
    let copy = Slot::new(&list_type);
    // SAFETY: the slots hold values of the layout until each drop or take.
    unsafe {
        list_type.clone_into(slot.ptr, copy.ptr);
        assert_ne!(copy.word(16), slot.word(16));
        list_type.drop_in_place(slot.ptr);
        assert_eq!(list_type.read(copy.ptr), value);
        list_type.drop_in_place(copy.ptr);
    }
}

#[test]
fn generic_values_read_back_with_their_type_arguments() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Cell".to_string(),
        class(&["T"], &[("value", Type::TypeParam("T".to_string()))]),
    );
    env.enums.insert(
        "Maybe".to_string(),
        enumeration(
            &["T"],
            &[
                ("Some", vec![Type::TypeParam("T".to_string())]),
                ("None", vec![]),
            ],
        ),
    );
    let cell_type = Type::Named("Cell".to_string(), vec![Type::named("int64")]);
    let descriptor = canonical_runtime_type_name(&cell_type);
    let cell = typed(&cell_type, &env);
    assert_eq!(cell.size(), 8);
    let slot = write(&cell, instance("Cell", vec![("value", int64(5))]));
    // SAFETY: the slot holds a value of the layout.
    let Value::Instance(read) = (unsafe { cell.take(slot.ptr) }) else {
        panic!("expected an instance");
    };
    assert_eq!(read.fields.map().get("value"), Some(&int64(5)));
    assert_eq!(
        read.fields.map().get(DIRECT_RUNTIME_TYPE_FIELD),
        Some(&Value::String(descriptor.clone()))
    );
    // A value that already names its type arguments writes the same way.
    assert_round_trip(&cell, Value::Instance(read));

    let maybe_type = Type::Named("Maybe".to_string(), vec![Type::named("int64")]);
    let maybe = typed(&maybe_type, &env);
    let slot = write(&maybe, variant("Maybe", "Some", vec![int64(1)]));
    // SAFETY: as above.
    let Value::EnumVariant(read) = (unsafe { maybe.take(slot.ptr) }) else {
        panic!("expected a variant");
    };
    let expected = format!(
        "Maybe{DIRECT_RUNTIME_TYPE_SEPARATOR}{}",
        canonical_runtime_type_name(&maybe_type)
    );
    assert_eq!(read.enum_name.as_str(), expected);
    assert_eq!(read.payloads, vec![int64(1)].into());
}

#[test]
fn values_of_another_shape_are_refused() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Point".to_string(),
        class(
            &[],
            &[("x", Type::named("int64")), ("y", Type::named("int64"))],
        ),
    );
    env.enums.insert(
        "Shape".to_string(),
        enumeration(&[], &[("Circle", vec![Type::named("int64")])]),
    );
    let point = typed(&Type::named("Point"), &env);
    let shape = typed(&Type::named("Shape"), &env);
    let pair = typed(
        &Type::Tuple(vec![Type::named("int64"), Type::named("int64")]),
        &env,
    );
    let union_type = union(vec![Type::named("int64"), Type::named("str")]);
    let either = typed(&union_type, &env);
    let cases = [
        (
            &point,
            instance("Other", vec![("x", int64(1)), ("y", int64(2))]),
        ),
        (&point, instance("Point", vec![("x", int64(1))])),
        (
            &point,
            instance("Point", vec![("x", int64(1)), ("z", int64(2))]),
        ),
        (
            &point,
            instance(
                "Point",
                vec![("x", int64(1)), ("y", int64(2)), ("z", int64(3))],
            ),
        ),
        (&point, int64(1)),
        (&shape, variant("Other", "Circle", vec![int64(1)])),
        (&shape, variant("Shape", "Square", vec![int64(1)])),
        (&shape, variant("Shape", "Circle", vec![])),
        (&pair, tuple(vec![Type::named("int64")], vec![int64(1)])),
        (&either, union_value(union_type.clone(), 5, int64(1))),
        (&either, int64(1)),
    ];
    for (typed, value) in cases {
        let slot = Slot::new(typed);
        let shown = format!("{value:?}");
        // SAFETY: the slot fits the layout.
        let error = unsafe { typed.write(value, slot.ptr) }.unwrap_err();
        assert!(
            error.contains("does not fit the layout of"),
            "{shown}: {error}"
        );
    }
}

#[test]
fn malformed_descriptors_are_refused() {
    assert!(TypedLayout::from_descriptor("not a descriptor").is_err());
    // A well-formed descriptor whose layout breaks a rule.
    let descriptor = encode_descriptor(&Type::named("int64"), &LayoutEnv::default())
        .expect("a layout")
        .replace("8;8;s", "4;4;s");
    assert!(TypedLayout::from_descriptor(&descriptor)
        .err()
        .is_some_and(|error| error.contains("wrong scalar size")));
}
