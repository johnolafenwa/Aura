use std::collections::BTreeMap;

use aura_core::types::Type;

use super::*;
use crate::c_layout::CField;
use crate::ffi::FfiScalar::{self, *};

fn layout(name: &str, union: bool, fields: Vec<(&str, CFieldKind)>) -> CStruct {
    CStruct {
        name: name.to_string(),
        fields: fields
            .into_iter()
            .map(|(name, kind)| CField {
                name: name.to_string(),
                kind,
            })
            .collect(),
        layout_controls: Default::default(),
        union,
    }
}

fn scalar(scalar: FfiScalar) -> CFieldKind {
    CFieldKind::Scalar(scalar)
}

fn array(kind: CFieldKind, count: usize) -> CFieldKind {
    CFieldKind::Array(Box::new(kind), count)
}

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn list(element: &str, values: Vec<Value>) -> Value {
    Value::Vec(VecValue::new(Type::named(element), values.into()))
}

fn instance(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Instance(InstanceValue {
        class_name: RuntimeName::from(name.to_string()),
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<BTreeMap<_, _>>()
            .into(),
    })
}

fn round_trip(value: &Value, layout: &CStruct) -> (Vec<u8>, Value) {
    let mut bytes = vec![0_u8; layout.size()];
    pack(value, layout, &mut bytes).expect("the value crosses");
    let back = unpack(layout, &bytes).expect("the bytes read back");
    (bytes, back)
}

fn field(value: &Value, name: &str) -> Value {
    let Value::Instance(instance) = value else {
        panic!("expected an instance, found {}", value.render());
    };
    instance.fields.get(name).expect("the field").into_owned()
}

#[test]
fn every_scalar_crosses_at_its_width() {
    let all = layout(
        "All",
        false,
        vec![
            ("flag", scalar(Bool)),
            ("a", scalar(I8)),
            ("b", scalar(I16)),
            ("c", scalar(I32)),
            ("d", scalar(I64)),
            ("e", scalar(U8)),
            ("f", scalar(U16)),
            ("g", scalar(U32)),
            ("h", scalar(U64)),
            ("x", scalar(F32)),
            ("y", scalar(F64)),
        ],
    );
    let value = instance(
        "All",
        vec![
            ("flag", Value::Bool(true)),
            ("a", int(-2)),
            ("b", int(-300)),
            ("c", int(-70_000)),
            ("d", int(i64::MIN)),
            ("e", int(200)),
            ("f", int(60_000)),
            ("g", int(4_000_000_000)),
            ("h", Value::Int(IntegerValue::from_u64(u64::MAX))),
            ("x", Value::Float(1.5)),
            ("y", Value::Float(-2.25)),
        ],
    );
    let (bytes, back) = round_trip(&value, &all);
    assert_eq!(bytes[0], 1);
    assert_eq!(bytes[1], 0xfe);
    assert_eq!(&bytes[2..4], &(-300_i16).to_le_bytes());
    assert_eq!(back.render(), value.render());
    assert_eq!(field(&back, "h").render(), u64::MAX.to_string());
}

#[test]
fn a_scalar_out_of_range_or_of_another_type_is_a_shape_error() {
    let byte = layout("Byte", false, vec![("v", scalar(U8))]);
    let mut bytes = [0_u8; 1];
    for bad in [int(256), int(-1), Value::Float(1.0), Value::Bool(true)] {
        let error = pack(&instance("Byte", vec![("v", bad)]), &byte, &mut bytes).unwrap_err();
        assert_eq!(error.code(), "AU4005");
        assert!(error.to_string().contains("uint8"), "{error}");
    }
    let small = layout("Small", false, vec![("v", scalar(I8))]);
    for bad in [int(128), int(-129), Value::Float(1.0)] {
        assert!(pack(&instance("Small", vec![("v", bad)]), &small, &mut bytes).is_err());
    }
    for (kind, bad) in [
        (Bool, int(1)),
        (F32, int(1)),
        (F64, int(1)),
        (U64, Value::Float(0.0)),
    ] {
        let one = layout("One", false, vec![("v", scalar(kind))]);
        let mut bytes = [0_u8; 8];
        assert!(pack(&instance("One", vec![("v", bad)]), &one, &mut bytes).is_err());
    }
    // A value that is not an instance, and one that lacks the field.
    assert!(pack(&int(1), &byte, &mut bytes).is_err());
    assert!(pack(&instance("Byte", vec![]), &byte, &mut bytes).is_err());
}

#[test]
fn a_bad_bool_byte_is_reported_when_read() {
    let flag = layout("Flag", false, vec![("on", scalar(Bool))]);
    let error = unpack(&flag, &[2]).unwrap_err();
    assert_eq!(error, CValueError::BadBool(2));
    assert_eq!(error.code(), "AU4001");
    assert!(error.to_string().contains("byte 2"));
}

#[test]
fn an_array_field_is_a_list_of_exactly_its_length() {
    let inner = layout("Point", false, vec![("x", scalar(F32)), ("y", scalar(F32))]);
    let outer = layout(
        "Shape",
        false,
        vec![
            ("tag", scalar(U8)),
            ("name", array(scalar(U8), 4)),
            ("weights", array(scalar(F32), 2)),
            ("corners", array(CFieldKind::Struct(inner.clone()), 2)),
            ("grid", array(array(scalar(I16), 2), 2)),
        ],
    );
    let point = |x: f64, y: f64| {
        instance(
            "Point",
            vec![("x", Value::Float(x)), ("y", Value::Float(y))],
        )
    };
    let value = instance(
        "Shape",
        vec![
            ("tag", int(7)),
            (
                "name",
                list("uint8", vec![int(65), int(66), int(67), int(0)]),
            ),
            (
                "weights",
                list("float32", vec![Value::Float(0.5), Value::Float(0.25)]),
            ),
            (
                "corners",
                list("Point", vec![point(1.0, 2.0), point(3.0, 4.0)]),
            ),
            (
                "grid",
                Value::Vec(VecValue::new(
                    Type::Named("list".to_string(), vec![Type::named("int16")]),
                    vec![
                        list("int16", vec![int(1), int(-2)]),
                        list("int16", vec![int(3), int(-4)]),
                    ]
                    .into(),
                )),
            ),
        ],
    );
    let (bytes, back) = round_trip(&value, &outer);
    assert_eq!(outer.size(), 40);
    assert_eq!(&bytes[..5], &[7, 65, 66, 67, 0]);
    assert_eq!(&bytes[8..12], &0.5_f32.to_le_bytes());
    assert_eq!(&bytes[16..20], &1.0_f32.to_le_bytes());
    assert_eq!(&bytes[36..38], &3_i16.to_le_bytes());
    assert_eq!(back.render(), value.render());
    let Value::Vec(weights) = field(&back, "weights") else {
        panic!("weights is a list");
    };
    assert!(weights.holds_float32());
    let Value::Vec(name) = field(&back, "name") else {
        panic!("name is a list");
    };
    assert_eq!(name.elements.as_uint8(), Some(&[65, 66, 67, 0][..]));

    // A list of another length does not cross.
    let short = instance(
        "Shape",
        vec![
            ("tag", int(7)),
            ("name", list("uint8", vec![int(65)])),
            ("weights", list("float32", vec![])),
            ("corners", list("Point", vec![])),
            ("grid", list("int16", vec![])),
        ],
    );
    let mut scratch = vec![0_u8; outer.size()];
    let error = pack(&short, &outer, &mut scratch).unwrap_err();
    assert_eq!(
        error,
        CValueError::Length {
            field: "name".to_string(),
            expected: 4,
            found: 1
        }
    );
    assert_eq!(error.code(), "AU4005");
    assert!(error.to_string().contains("`name` holds 4"));
    // A field that is not a list does not cross.
    let wrong = instance("Shape", vec![("tag", int(7)), ("name", int(1))]);
    let error = pack(&wrong, &outer, &mut scratch).unwrap_err();
    assert!(error.to_string().contains("list[uint8]"), "{error}");
}

#[test]
fn a_union_is_its_bytes() {
    let event = layout(
        "Event",
        true,
        vec![
            ("kind", scalar(U32)),
            ("ratio", scalar(F64)),
            ("text", array(scalar(U8), 12)),
        ],
    );
    assert_eq!(event.size(), 16);
    let bytes: Vec<u8> = (1..=16).collect();
    let value = unpack(&event, &bytes).expect("a union reads");
    let Value::Vec(held) = field(&value, UNION_BYTES_FIELD) else {
        panic!("a union holds its bytes");
    };
    assert_eq!(held.elements.as_uint8(), Some(&bytes[..]));
    let mut again = vec![0_u8; 16];
    pack(&value, &event, &mut again).expect("a union writes");
    assert_eq!(again, bytes);

    // Members read the leading bytes.
    let kind = union_member(&scalar(U32), &bytes).expect("kind");
    assert_eq!(kind.render(), u32::from_le_bytes([1, 2, 3, 4]).to_string());
    let text = union_member(&array(scalar(U8), 12), &bytes).expect("text");
    assert_eq!(
        text.render(),
        list("uint8", (1..=12).map(int).collect()).render()
    );
    assert!(union_member(&scalar(Bool), &bytes).is_ok());
    assert!(union_member(&scalar(Bool), &bytes[1..]).is_err());

    // Storing a member zeroes the rest.
    let stored = union_with_member(&Value::Float(1.0), &scalar(F64), "ratio", 16).expect("stored");
    assert_eq!(&stored[..8], &1.0_f64.to_le_bytes());
    assert_eq!(&stored[8..], &[0; 8]);
    assert!(union_with_member(&int(1), &scalar(F64), "ratio", 16).is_err());

    // A union of the wrong size, or with no bytes, does not cross.
    let short = instance(
        "Event",
        vec![(UNION_BYTES_FIELD, list("uint8", vec![int(1)]))],
    );
    let error = pack(&short, &event, &mut again).unwrap_err();
    assert_eq!(
        error,
        CValueError::UnionLength {
            name: "Event".to_string(),
            expected: 16,
            found: 1
        }
    );
    assert_eq!(error.code(), "AU4005");
    assert!(error
        .to_string()
        .contains("C union `Event` is 16 byte(s), but its `bytes` list has 1"));
    assert!(pack(&instance("Event", vec![]), &event, &mut again).is_err());

    // A union as a field and as an array element.
    let holder = layout(
        "Holder",
        false,
        vec![
            ("tag", scalar(I32)),
            ("event", CFieldKind::Struct(event.clone())),
            ("more", array(CFieldKind::Struct(event.clone()), 2)),
        ],
    );
    let held = instance(
        "Holder",
        vec![
            ("tag", int(-1)),
            ("event", value.clone()),
            ("more", list("Event", vec![value.clone(), value.clone()])),
        ],
    );
    let (bytes, back) = round_trip(&held, &holder);
    assert_eq!(bytes.len(), 56);
    assert_eq!(bytes[8], 1);
    assert_eq!(back.render(), held.render());
}

#[test]
fn a_struct_array_crosses_element_by_element() {
    let point = layout("Point", false, vec![("x", scalar(F32)), ("y", scalar(I32))]);
    let value = list(
        "Point",
        vec![
            instance("Point", vec![("x", Value::Float(1.0)), ("y", int(2))]),
            instance("Point", vec![("x", Value::Float(3.0)), ("y", int(4))]),
        ],
    );
    let bytes = pack_elements(&value, &point).expect("the list crosses");
    assert_eq!(bytes.len(), 16);
    assert_eq!(&bytes[12..], &4_i32.to_le_bytes());
    let back = unpack_elements(&point, &bytes).expect("the array reads back");
    assert_eq!(back.render(), value.render());
    assert_eq!(
        pack_elements(&list("Point", vec![]), &point),
        Ok(Vec::new())
    );
    assert!(pack_elements(&int(1), &point).is_err());
    assert!(pack_elements(&list("Point", vec![int(1)]), &point).is_err());

    assert_eq!(
        kind_type(&array(CFieldKind::Struct(point), 3)).to_string(),
        "list[Point]"
    );
}

#[test]
fn pack_elements_rejects_overflowing_layout() {
    let overflowing = layout(
        "Huge",
        false,
        vec![("data", array(scalar(U64), usize::MAX / 4))],
    );
    let value = list("Huge", vec![instance("Huge", vec![])]);
    let error = pack_elements(&value, &overflowing).expect_err("overflowing layout must fail");
    assert_eq!(error.code(), "AU4005");
    assert!(error.to_string().contains("overflows"), "{error}");
}

#[test]
fn bit_field_pack_checks_width_and_keeps_the_other_bits() {
    let mut record = layout(
        "Tiny",
        false,
        vec![("tag", CFieldKind::Scalar(FfiScalar::U8))],
    );
    record.layout_controls.bit_widths.insert("tag".into(), 3);
    let mut value = unpack(&record, &[0xf5]).unwrap();
    let mut bytes = [0xa0];
    pack(&value, &record, &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5]);
    let Value::Instance(instance) = &mut value else {
        panic!("expected a Tiny")
    };
    instance
        .fields
        .insert("tag", Value::Int(IntegerValue::from_u64(8)))
        .unwrap();
    let problem = pack(&value, &record, &mut bytes).unwrap_err();
    assert_eq!(problem.code(), "AU4033");
    assert!(problem.to_string().contains("3-bit"));
}

#[test]
fn fixed_array_wide_element_codecs_preserve_all_bits() {
    for (name, integer, bits) in [
        (
            "int128",
            IntegerValue::from_signed(i128::MIN),
            i128::MIN as u128,
        ),
        ("uint128", IntegerValue::from_literal(u128::MAX), u128::MAX),
    ] {
        let kind = CFieldKind::Struct(layout(
            &format!("#fixed.{name}"),
            false,
            vec![("low", scalar(U64)), ("high", scalar(U64))],
        ));
        let mut bytes = [0; 16];
        assert_eq!(kind_type(&kind), Type::named(name));
        pack_kind(&Value::Int(integer), &kind, "words", &mut bytes).unwrap();
        assert_eq!(u128::from_le_bytes(bytes), bits);
        let Value::Int(round_trip) = unpack_kind(&kind, &bytes).unwrap() else {
            panic!("expected integer")
        };
        assert_eq!(round_trip, integer);
        assert!(matches!(
            pack_kind(&Value::Bool(false), &kind, "words", &mut bytes),
            Err(CValueError::Shape { .. })
        ));
    }
}
