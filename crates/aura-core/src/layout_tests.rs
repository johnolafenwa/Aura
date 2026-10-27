use std::collections::BTreeMap;

use super::*;

fn named(name: &str) -> Type {
    Type::named(name)
}

fn generic(name: &str, args: Vec<Type>) -> Type {
    Type::Named(name.to_string(), args)
}

fn union(members: Vec<Type>) -> Type {
    Type::normalize_union(members, "main", &BTreeMap::new()).expect("a valid union")
}

fn class(fields: &[(&str, Type)]) -> ClassDefinition {
    ClassDefinition {
        type_params: Vec::new(),
        fields: fields
            .iter()
            .map(|(name, ty)| (name.to_string(), ty.clone()))
            .collect(),
    }
}

fn enumeration(variants: &[(&str, Vec<Type>)]) -> EnumDefinition {
    EnumDefinition {
        type_params: Vec::new(),
        variants: variants
            .iter()
            .map(|(name, payloads)| (name.to_string(), payloads.clone()))
            .collect(),
    }
}

fn layout_of(env: &LayoutEnv, ty: &Type) -> (LayoutTable, LayoutId) {
    let mut builder = LayoutBuilder::new(env);
    let id = builder.layout(ty).expect("a layout");
    let table = builder.into_table();
    table.validate().expect("a valid table");
    (table, id)
}

fn fields(table: &LayoutTable, id: LayoutId) -> Vec<(String, u64, LayoutKind)> {
    match &table.get(id).kind {
        LayoutKind::Struct { fields, .. } => fields
            .iter()
            .map(|field| {
                (
                    field.name.clone(),
                    field.offset,
                    table.get(field.layout).kind.clone(),
                )
            })
            .collect(),
        other => panic!("expected a struct, found {other:?}"),
    }
}

#[test]
fn scalars_take_their_width() {
    let env = LayoutEnv::default();
    for (name, size) in [
        ("bool", 1),
        ("int8", 1),
        ("uint8", 1),
        ("int16", 2),
        ("uint16", 2),
        ("int32", 4),
        ("uint32", 4),
        ("float32", 4),
        ("int64", 8),
        ("uint64", 8),
        ("float64", 8),
        ("intsize", 8),
        ("uintsize", 8),
        ("int128", 16),
        ("uint128", 16),
        ("Duration", 16),
    ] {
        let (table, id) = layout_of(&env, &named(name));
        let layout = table.get(id);
        assert_eq!((layout.size, layout.align), (size, size), "{name}");
        let LayoutKind::Scalar(scalar) = layout.kind else {
            panic!("{name} is not a scalar");
        };
        assert_eq!(scalar.name(), name);
        assert_eq!(scalar.size(), size);
        assert_eq!(
            scalar.is_integer(),
            !matches!(name, "bool" | "float32" | "float64" | "Duration")
        );
    }
    let (table, id) = layout_of(&env, &Type::Unit);
    assert_eq!(table.get(id).size, 0);
    assert_eq!(table.get(id).kind, LayoutKind::Unit);
}

#[test]
fn runtime_objects_and_type_parameters_are_one_pointer() {
    let env = LayoutEnv::default();
    for ty in [
        named("str"),
        generic("list", vec![named("int64")]),
        generic("dict", vec![named("str"), named("int64")]),
        Type::TypeParam("T".to_string()),
        Type::Module("json".to_string()),
        Type::Function {
            params: Vec::new(),
            return_type: Box::new(Type::Unit),
        },
        Type::Tuple(vec![Type::TypeParam("T".to_string()), named("int64")]),
    ] {
        let (table, id) = layout_of(&env, &ty);
        assert_eq!(table.get(id).kind, LayoutKind::Handle, "{ty}");
        assert_eq!(table.get(id).size, POINTER_BYTES);
        assert!(table.needs_drop(id));
    }
}

#[test]
fn a_returned_view_has_no_layout() {
    let env = LayoutEnv::default();
    let view = Type::ReturnedView(Box::new(crate::types::ReturnedViewType {
        mutable: false,
        pointee: named("int64"),
        origin: 0,
    }));
    let error = LayoutBuilder::new(&env).layout(&view).unwrap_err();
    assert!(error.to_string().contains("returned view"), "{error}");
}

#[test]
fn classes_and_tuples_lay_fields_out_in_order() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Mixed".to_string(),
        class(&[
            ("flag", named("bool")),
            ("count", named("int64")),
            ("small", named("int32")),
            ("name", named("str")),
        ]),
    );
    let (table, id) = layout_of(&env, &named("Mixed"));
    let layout = table.get(id);
    assert_eq!((layout.size, layout.align), (32, 8));
    let offsets = fields(&table, id)
        .into_iter()
        .map(|(name, offset, _)| (name, offset))
        .collect::<Vec<_>>();
    assert_eq!(
        offsets,
        vec![
            ("flag".to_string(), 0),
            ("count".to_string(), 8),
            ("small".to_string(), 16),
            ("name".to_string(), 24),
        ]
    );
    assert!(matches!(
        &layout.kind,
        LayoutKind::Struct { class: Some(name), .. } if name == "Mixed"
    ));
    assert!(table.needs_drop(id));
    assert!(!table.holds_block(id));

    let (table, id) = layout_of(&env, &Type::Tuple(vec![named("int32"), named("bool")]));
    assert_eq!((table.get(id).size, table.get(id).align), (8, 4));
    assert_eq!(
        fields(&table, id)
            .into_iter()
            .map(|(name, offset, _)| (name, offset))
            .collect::<Vec<_>>(),
        vec![("0".to_string(), 0), ("1".to_string(), 4)]
    );
    assert!(!table.needs_drop(id));

    let (table, id) = layout_of(&env, &Type::Tuple(Vec::new()));
    assert_eq!((table.get(id).size, table.get(id).align), (0, 1));
}

#[test]
fn copy_classes_nest_inline() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Vec3".to_string(),
        class(&[
            ("x", named("float64")),
            ("y", named("float64")),
            ("z", named("float64")),
        ]),
    );
    env.classes.insert(
        "Ray".to_string(),
        class(&[("origin", named("Vec3")), ("direction", named("Vec3"))]),
    );
    let (table, id) = layout_of(&env, &named("Ray"));
    assert_eq!(table.get(id).size, 48);
    let laid = fields(&table, id);
    assert_eq!(laid[1].1, 24);
    assert!(matches!(&laid[1].2, LayoutKind::Struct { class: Some(name), .. } if name == "Vec3"));
    assert!(!table.needs_drop(id));
}

#[test]
fn enums_put_payloads_after_the_tag_and_overlap_variants() {
    let mut env = LayoutEnv::default();
    env.enums.insert(
        "Shape".to_string(),
        enumeration(&[
            ("Circle", vec![named("float64")]),
            ("Rect", vec![named("float64"), named("float64")]),
            ("Empty", Vec::new()),
        ]),
    );
    let (table, id) = layout_of(&env, &named("Shape"));
    let layout = table.get(id);
    assert_eq!((layout.size, layout.align), (24, 8));
    let LayoutKind::Tagged {
        enum_name,
        tag_width,
        payload_offset,
        variants,
    } = &layout.kind
    else {
        panic!("expected a tagged layout");
    };
    assert_eq!(enum_name.as_deref(), Some("Shape"));
    assert_eq!((*tag_width, *payload_offset), (1, 8));
    let names = variants
        .iter()
        .map(|variant| variant.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Circle", "Rect", "Empty"]);
    assert_eq!(variants[1].fields[1].offset, 16);
    assert!(variants[2].fields.is_empty());
    assert!(!table.needs_drop(id));

    env.enums.insert(
        "Color".to_string(),
        enumeration(&[("Red", Vec::new()), ("Green", Vec::new())]),
    );
    let (table, id) = layout_of(&env, &named("Color"));
    assert_eq!((table.get(id).size, table.get(id).align), (1, 1));
}

#[test]
fn a_union_is_a_tag_and_its_widest_member() {
    let env = LayoutEnv::default();
    let optional = union(vec![named("int64"), Type::Unit]);
    let (table, id) = layout_of(&env, &optional);
    let layout = table.get(id);
    assert_eq!((layout.size, layout.align), (16, 8));
    let LayoutKind::Tagged {
        enum_name,
        variants,
        ..
    } = &layout.kind
    else {
        panic!("expected a tagged layout");
    };
    assert!(enum_name.is_none());
    let Type::Union(members) = &optional else {
        panic!("expected a union");
    };
    for (variant, key) in variants.iter().zip(&members.keys) {
        assert_eq!(&variant.name, key);
        assert_eq!(variant.fields.len(), 1);
        assert_eq!(variant.fields[0].offset, 8);
    }
}

#[test]
fn a_self_recursive_enum_holds_itself_behind_a_pointer() {
    let mut env = LayoutEnv::default();
    env.enums.insert(
        "Tree".to_string(),
        enumeration(&[
            ("Node", vec![named("Tree"), named("Tree")]),
            ("Leaf", vec![named("int64")]),
        ]),
    );
    let (table, id) = layout_of(&env, &named("Tree"));
    assert_eq!(table.get(id).size, 24);
    let LayoutKind::Tagged { variants, .. } = &table.get(id).kind else {
        panic!("expected a tagged layout");
    };
    for field in &variants[0].fields {
        assert_eq!(table.get(field.layout).kind, LayoutKind::Indirect(id));
    }
    assert_eq!(
        table.get(variants[1].fields[0].layout).kind,
        LayoutKind::Scalar(ScalarLayout::Int64)
    );
    assert!(table.needs_drop(id));
    assert!(
        table.render(id).contains("indirect(^"),
        "{}",
        table.render(id)
    );
}

#[test]
fn a_linked_class_stores_its_optional_next_node_behind_a_pointer() {
    let mut env = LayoutEnv::default();
    let next = union(vec![named("Node"), Type::Unit]);
    env.classes.insert(
        "Node".to_string(),
        class(&[("value", named("int64")), ("next", next.clone())]),
    );
    let mut builder = LayoutBuilder::new(&env);
    let node = builder.layout(&named("Node")).expect("a layout");
    let plain_next = builder.layout(&next).expect("a layout");
    let table = builder.into_table();
    table.validate().expect("a valid table");
    // Inside `Node`, the optional next node is a tag and one pointer.
    assert_eq!(table.get(node).size, 24);
    let laid = fields(&table, node);
    let LayoutKind::Struct {
        fields: node_fields,
        ..
    } = &table.get(node).kind
    else {
        panic!("expected a struct");
    };
    let stored_next = node_fields[1].layout;
    assert_ne!(stored_next, plain_next);
    assert_eq!(laid[1].1, 8);
    let LayoutKind::Tagged { variants, .. } = &table.get(stored_next).kind else {
        panic!("expected a tagged layout");
    };
    assert!(variants
        .iter()
        .any(|variant| table.get(variant.fields[0].layout).kind == LayoutKind::Indirect(node)));
    // Anywhere else, `Node | None` keeps the node inline.
    assert_eq!(table.get(plain_next).size, 32);
    let LayoutKind::Tagged { variants, .. } = &table.get(plain_next).kind else {
        panic!("expected a tagged layout");
    };
    assert!(variants
        .iter()
        .any(|variant| variant.fields[0].layout == node));
    // A node holds a block, through its optional next node; the int alone
    // holds none.
    assert!(table.holds_block(node));
    assert!(table.holds_block(plain_next));
    assert!(!table.holds_block(node_fields[0].layout));
}

#[test]
fn mutual_recursion_lays_out_the_same_from_either_end() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "A".to_string(),
        class(&[
            ("b", union(vec![named("B"), Type::Unit])),
            ("x", named("int64")),
        ]),
    );
    env.classes.insert(
        "B".to_string(),
        class(&[
            ("a", union(vec![named("A"), Type::Unit])),
            ("y", named("bool")),
        ]),
    );
    let (from_a, a) = layout_of(&env, &named("A"));
    let (from_b, b) = layout_of(&env, &named("B"));
    let mut builder = LayoutBuilder::new(&env);
    let b_after_a = {
        builder.layout(&named("A")).expect("a layout");
        builder.layout(&named("B")).expect("a layout")
    };
    let a_after_b = {
        let mut other = LayoutBuilder::new(&env);
        other.layout(&named("B")).expect("a layout");
        let id = other.layout(&named("A")).expect("a layout");
        other.into_table().render(id)
    };
    assert_eq!(builder.table().render(b_after_a), from_b.render(b));
    assert_eq!(a_after_b, from_a.render(a));
    // Each holds the other behind a pointer, so neither is infinite.
    assert_eq!(from_a.get(a).size, 24);
    assert_eq!(from_b.get(b).size, 24);
}

#[test]
fn recursion_through_a_generic_class_points_at_every_member_of_the_cycle() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Pair".to_string(),
        ClassDefinition {
            type_params: vec!["T".to_string()],
            fields: vec![
                ("first".to_string(), Type::TypeParam("T".to_string())),
                ("second".to_string(), Type::TypeParam("T".to_string())),
            ],
        },
    );
    env.classes.insert(
        "Node".to_string(),
        class(&[(
            "kids",
            union(vec![generic("Pair", vec![named("Node")]), Type::Unit]),
        )]),
    );
    let (table, node) = layout_of(&env, &named("Node"));
    assert_eq!(table.get(node).size, 16);
    let (table, pair) = layout_of(&env, &generic("Pair", vec![named("Node")]));
    for (_, _, kind) in fields(&table, pair) {
        assert!(matches!(kind, LayoutKind::Indirect(_)));
    }
    // An instantiation outside the cycle stays inline.
    let (table, pair) = layout_of(&env, &generic("Pair", vec![named("int32")]));
    assert_eq!(table.get(pair).size, 8);
    assert!(!table.needs_drop(pair));
    let (table, pair) = layout_of(&env, &generic("Pair", vec![named("str")]));
    for (_, _, kind) in fields(&table, pair) {
        assert_eq!(kind, LayoutKind::Handle);
    }
}

#[test]
fn a_recursive_tuple_inside_a_cycle_gets_its_own_storage_layout() {
    let mut env = LayoutEnv::default();
    let pair = Type::Tuple(vec![named("List"), named("int64")]);
    env.classes.insert(
        "List".to_string(),
        class(&[("rest", union(vec![pair.clone(), Type::Unit]))]),
    );
    let mut builder = LayoutBuilder::new(&env);
    let list = builder.layout(&named("List")).expect("a layout");
    let plain_pair = builder.layout(&pair).expect("a layout");
    let table = builder.into_table();
    table.validate().expect("a valid table");
    assert_eq!(table.get(plain_pair).size, table.get(list).size + 8);
    let rendered = table.render(list);
    assert!(rendered.contains("indirect(^"), "{rendered}");
}

#[test]
fn a_type_that_grows_without_bound_stays_a_handle() {
    let mut env = LayoutEnv::default();
    let t = Type::TypeParam("T".to_string());
    env.classes.insert(
        "Grow".to_string(),
        ClassDefinition {
            type_params: vec!["T".to_string()],
            fields: vec![
                ("value".to_string(), t.clone()),
                (
                    "next".to_string(),
                    union(vec![
                        generic("Grow", vec![generic("list", vec![t])]),
                        Type::Unit,
                    ]),
                ),
            ],
        },
    );
    env.classes.insert(
        "Holder".to_string(),
        class(&[("grow", generic("Grow", vec![named("int64")]))]),
    );
    let (table, id) = layout_of(&env, &generic("Grow", vec![named("int64")]));
    assert_eq!(table.get(id).kind, LayoutKind::Handle);
    // A type that holds it reaches as much, so it stays a handle too,
    // whichever of the two a walk meets first.
    let mut builder = LayoutBuilder::new(&env);
    builder
        .layout(&generic("Grow", vec![named("int64")]))
        .expect("a layout");
    let holder = builder.layout(&named("Holder")).expect("a layout");
    assert_eq!(builder.table().get(holder).kind, LayoutKind::Handle);
    let (table, holder) = layout_of(&env, &named("Holder"));
    assert_eq!(table.get(holder).kind, LayoutKind::Handle);
}

#[test]
fn a_class_that_reaches_too_many_others_stays_a_handle() {
    let mut env = LayoutEnv::default();
    let count = MAX_LAYOUT_INSTANTIATIONS + 10;
    for index in 0..count {
        let fields = if index + 1 < count {
            vec![("next", named(&format!("C{}", index + 1)))]
        } else {
            vec![("value", named("int64"))]
        };
        env.classes.insert(format!("C{index}"), class(&fields));
    }
    // `C20` reaches fewer classes than the budget, so it is inline.
    let (table, id) = layout_of(&env, &named("C20"));
    assert!(matches!(table.get(id).kind, LayoutKind::Struct { .. }));
    assert_eq!(table.get(id).size, 8);
    // `C0` reaches more, so it stays a handle, whichever is laid out first.
    let (table, id) = layout_of(&env, &named("C0"));
    assert_eq!(table.get(id).kind, LayoutKind::Handle);
    let mut builder = LayoutBuilder::new(&env);
    builder.layout(&named("C20")).expect("a layout");
    let first = builder.layout(&named("C0")).expect("a layout");
    assert_eq!(builder.table().get(first).kind, LayoutKind::Handle);
}

#[test]
fn a_class_with_the_wrong_type_argument_count_is_a_handle() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Box".to_string(),
        ClassDefinition {
            type_params: vec!["T".to_string()],
            fields: vec![("value".to_string(), Type::TypeParam("T".to_string()))],
        },
    );
    env.enums.insert(
        "Maybe".to_string(),
        EnumDefinition {
            type_params: vec!["T".to_string()],
            variants: vec![
                ("Some".to_string(), vec![Type::TypeParam("T".to_string())]),
                ("Nothing".to_string(), Vec::new()),
            ],
        },
    );
    for ty in [named("Box"), named("Maybe"), named("Unknown")] {
        let (table, id) = layout_of(&env, &ty);
        assert_eq!(table.get(id).kind, LayoutKind::Handle, "{ty}");
    }
    let (table, id) = layout_of(&env, &generic("Maybe", vec![named("int32")]));
    assert_eq!((table.get(id).size, table.get(id).align), (8, 4));
}

#[test]
fn many_variants_widen_the_tag() {
    assert_eq!(tag_width_for(1), 1);
    assert_eq!(tag_width_for(256), 1);
    assert_eq!(tag_width_for(257), 2);
    assert_eq!(tag_width_for(65_536), 2);
    assert_eq!(tag_width_for(65_537), 4);
    let mut env = LayoutEnv::default();
    let variants = (0..300)
        .map(|index| (format!("V{index}"), Vec::new()))
        .collect();
    env.enums.insert(
        "Wide".to_string(),
        EnumDefinition {
            type_params: Vec::new(),
            variants,
        },
    );
    let (table, id) = layout_of(&env, &named("Wide"));
    assert_eq!((table.get(id).size, table.get(id).align), (2, 2));
    env.enums
        .insert("Never".to_string(), EnumDefinition::default());
    let (table, id) = layout_of(&env, &named("Never"));
    assert_eq!(table.get(id).size, 1);
}

#[test]
fn validate_refuses_broken_tables() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Point".to_string(),
        class(&[("x", named("int32")), ("y", named("int64"))]),
    );
    env.enums.insert(
        "Shape".to_string(),
        enumeration(&[("Dot", vec![named("int64")])]),
    );
    let (table, point) = layout_of(&env, &named("Point"));
    let (enum_table, shape) = layout_of(&env, &named("Shape"));
    let broken = |edit: &dyn Fn(&mut LayoutTable), table: &LayoutTable| {
        let mut table = table.clone();
        edit(&mut table);
        table.validate().unwrap_err().to_string()
    };
    let point_index = point.index();
    let shape_index = shape.index();
    assert!(broken(&|table| table.layouts[point_index].align = 3, &table).contains("power of two"));
    assert!(
        broken(&|table| table.layouts[point_index].size = 12, &table)
            .contains("multiple of its alignment")
    );
    assert!(broken(
        &|table| {
            if let LayoutKind::Struct { fields, .. } = &mut table.layouts[point_index].kind {
                fields[1].offset = 4;
            }
        },
        &table
    )
    .contains("misaligns"));
    assert!(broken(
        &|table| {
            if let LayoutKind::Struct { fields, .. } = &mut table.layouts[point_index].kind {
                fields[1].offset = 0;
            }
        },
        &table
    )
    .contains("overlaps"));
    assert!(broken(
        &|table| {
            if let LayoutKind::Struct { fields, .. } = &mut table.layouts[point_index].kind {
                fields[1].offset = 16;
            }
        },
        &table
    )
    .contains("outside"));
    assert!(broken(
        &|table| {
            if let LayoutKind::Struct { fields, .. } = &mut table.layouts[point_index].kind {
                fields[1].layout = LayoutId(99);
            }
        },
        &table
    )
    .contains("missing layout"));
    assert!(broken(
        &|table| table.layouts[point_index].kind = LayoutKind::Unit,
        &table
    )
    .contains("unit with bytes"));
    assert!(broken(
        &|table| table.layouts[point_index].kind = LayoutKind::Scalar(ScalarLayout::Int8),
        &table
    )
    .contains("scalar size"));
    assert!(broken(
        &|table| table.layouts[point_index].kind = LayoutKind::Handle,
        &table
    )
    .contains("not one pointer"));
    assert!(broken(
        &|table| {
            table.layouts[point_index].size = 8;
            table.layouts[point_index].kind = LayoutKind::Indirect(LayoutId(99));
        },
        &table
    )
    .contains("missing layout"));
    assert!(broken(
        &|table| table.layouts[point_index].kind = LayoutKind::Indirect(LayoutId(0)),
        &table
    )
    .contains("not one pointer"));
    let tagged = |tag: u8, payload: u64, variants: usize| {
        move |table: &mut LayoutTable| {
            if let LayoutKind::Tagged {
                tag_width,
                payload_offset,
                variants: laid,
                ..
            } = &mut table.layouts[shape_index].kind
            {
                *tag_width = tag;
                *payload_offset = payload;
                let first = laid[0].clone();
                laid.resize(variants, first);
            }
        }
    };
    assert!(broken(&tagged(3, 8, 1), &enum_table).contains("1, 2, or 4"));
    assert!(broken(&tagged(4, 2, 1), &enum_table).contains("overlaps its tag"));
    assert!(broken(&tagged(2, 8, 1), &enum_table).contains("too narrow or wide"));
}

#[test]
fn descriptors_carry_the_definitions_a_layout_reaches() {
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Pair".to_string(),
        ClassDefinition {
            type_params: vec!["T".to_string()],
            fields: vec![
                ("first".to_string(), Type::TypeParam("T".to_string())),
                ("second".to_string(), Type::TypeParam("T".to_string())),
            ],
        },
    );
    env.enums.insert(
        "Tree".to_string(),
        enumeration(&[
            ("Node", vec![generic("Pair", vec![named("Tree")])]),
            ("Leaf", vec![named("str")]),
        ]),
    );
    env.classes
        .insert("Unused".to_string(), class(&[("x", named("int64"))]));
    let ty = Type::Tuple(vec![named("Tree"), named("int32")]);
    let descriptor = encode_descriptor(&ty, &env);
    assert!(descriptor.starts_with(LAYOUT_DESCRIPTOR_PREFIX));
    let (decoded, decoded_env) = decode_descriptor(&descriptor).expect("a descriptor");
    assert_eq!(decoded, ty);
    assert!(decoded_env.classes.contains_key("Pair"));
    assert!(decoded_env.enums.contains_key("Tree"));
    assert!(!decoded_env.classes.contains_key("Unused"));
    let (original, original_id) = layout_of(&env, &ty);
    let (rebuilt, rebuilt_id) = layout_of(&decoded_env, &decoded);
    assert_eq!(original.render(original_id), rebuilt.render(rebuilt_id));
}

#[test]
fn decoding_refuses_malformed_descriptors() {
    let env = LayoutEnv::default();
    let good = encode_descriptor(&named("int64"), &env);
    for (descriptor, message) in [
        ("aura-layout-v0:u0;0;".to_string(), "unknown version"),
        (format!("{good}x"), "trailing bytes"),
        (
            format!("{LAYOUT_DESCRIPTOR_PREFIX}u2;1:A0;0;1:A0;0;0;"),
            "defines class `A` twice",
        ),
        (
            format!("{LAYOUT_DESCRIPTOR_PREFIX}u1;1:A0;0;1;1:A0;0;"),
            "defines `A` twice",
        ),
        (
            format!("{LAYOUT_DESCRIPTOR_PREFIX}u0;2;1:E0;0;1:E0;0;"),
            "defines `E` twice",
        ),
        (format!("{LAYOUT_DESCRIPTOR_PREFIX}u1;5:AB"), "ends early"),
    ] {
        let error = decode_descriptor(&descriptor).unwrap_err();
        assert!(error.contains(message), "{descriptor}: {error}");
    }
    let mut env = LayoutEnv::default();
    env.classes.insert(
        "Box".to_string(),
        ClassDefinition {
            type_params: vec!["T".to_string()],
            fields: vec![("value".to_string(), Type::TypeParam("T".to_string()))],
        },
    );
    env.enums.insert(
        "Shape".to_string(),
        enumeration(&[("Dot", vec![named("int64")]), ("None", Vec::new())]),
    );
    let ty = Type::Tuple(vec![generic("Box", vec![named("Shape")]), named("Shape")]);
    let descriptor = encode_descriptor(&ty, &env);
    assert_eq!(decode_descriptor(&descriptor).expect("a descriptor").1, env);
}

#[test]
fn strongly_connected_components_split_a_graph() {
    // 0 -> 1 -> 2 -> 0, 2 -> 3, 3 -> 3, 4 alone.
    let edges = vec![vec![1], vec![2], vec![0, 3], vec![3], vec![]];
    let mut components = strongly_connected_components(&edges);
    components.sort();
    assert_eq!(components, vec![vec![0, 1, 2], vec![3], vec![4]]);
}
