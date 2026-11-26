use super::*;
use crate::c_layout::{CField, CFieldKind, CStruct, LayoutControls};
use crate::ffi::FfiScalar;

#[test]
fn fixed_array_boxing_descriptor_preserves_class_fields_and_scalar_signedness() {
    let scalars = [
        ("int8", FfiScalar::I8),
        ("int16", FfiScalar::I16),
        ("int32", FfiScalar::I32),
        ("int64", FfiScalar::I64),
        ("uint8", FfiScalar::U8),
        ("uint16", FfiScalar::U16),
        ("uint32", FfiScalar::U32),
        ("uint64", FfiScalar::U64),
        ("float32", FfiScalar::F32),
        ("float64", FfiScalar::F64),
        ("bool", FfiScalar::Bool),
    ];
    let class = DirectType::PlainClass(PlainClassType {
        c_layout: None,
        class_name: "Sample".into(),
        fields: scalars
            .iter()
            .map(|(name, _)| PlainClassField {
                name: format!("sample_{name}"),
                ty: DirectType::Scalar(ScalarKind::of_name(name).unwrap()),
            })
            .collect(),
    });
    let descriptor = FixedArrayLayout::field_descriptor(&class, Some(3)).unwrap();
    let outer: CStruct = serde_json::from_slice(&descriptor).unwrap();
    let [CField {
        name,
        kind: CFieldKind::Array(element, 3),
    }] = outer.fields.as_slice()
    else {
        panic!("expected one array of three samples");
    };
    assert_eq!(name, "elements");
    let CFieldKind::Struct(sample) = element.as_ref() else {
        panic!("expected sample record");
    };
    assert_eq!(sample.name, "Sample");
    let expected = scalars.map(|(name, scalar)| CField {
        name: format!("sample_{name}"),
        kind: CFieldKind::Scalar(scalar),
    });
    assert_eq!(sample.fields.as_slice(), &expected);
    assert_eq!(outer.size(), 3 * sample.size());
}

#[test]
fn fixed_array_boxing_descriptor_preserves_nested_packed_wire_layout() {
    let wire = CStruct {
        name: "Wire".into(),
        union: false,
        layout_controls: Box::new(LayoutControls {
            packed: true,
            ..Default::default()
        }),
        fields: vec![
            CField {
                name: "tag".into(),
                kind: CFieldKind::Scalar(FfiScalar::U8),
            },
            CField {
                name: "word".into(),
                kind: CFieldKind::Scalar(FfiScalar::U32),
            },
        ],
    };
    let element = DirectType::PlainClass(PlainClassType {
        c_layout: Some(std::sync::Arc::new(wire.clone())),
        class_name: wire.name.clone(),
        fields: Vec::new(),
    });
    let row = DirectType::Fixed(FixedArrayLayout {
        ty: Type::Named("fixed".into(), vec![Type::named("Wire"), Type::named("2")]),
        bytes: 10,
        align: 1,
        exact_align: 1,
        element: Box::new(element),
        count: 2,
    });
    let descriptor = FixedArrayLayout::field_descriptor(&row, Some(3)).unwrap();
    let outer: CStruct = serde_json::from_slice(&descriptor).unwrap();
    let row_kind = CFieldKind::Array(Box::new(CFieldKind::Struct(wire)), 2);
    assert_eq!(
        outer.fields[0].kind,
        CFieldKind::Array(Box::new(row_kind), 3)
    );
    assert_eq!((outer.size(), outer.align()), (30, 1));
}
