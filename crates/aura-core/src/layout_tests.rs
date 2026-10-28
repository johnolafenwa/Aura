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
    assert!(broken(
        &|table| {
            table.layouts[shape_index].size = 0;
            table.layouts[shape_index].align = 1;
            if let LayoutKind::Tagged {
                tag_width,
                payload_offset,
                variants,
                ..
            } = &mut table.layouts[shape_index].kind
            {
                *tag_width = 1;
                *payload_offset = 1;
                variants.clear();
            }
        },
        &enum_table
    )
    .contains("aligned room"));
}

#[test]
fn descriptors_carry_the_layouts_a_value_reaches() {
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
    let descriptor = encode_descriptor(&ty, &env).expect("a layout");
    assert!(descriptor.starts_with(LAYOUT_DESCRIPTOR_PREFIX));
    let (decoded, root) = decode_descriptor(&descriptor).expect("a descriptor");
    assert_eq!(decoded.get(root).ty, ty);
    let (original, original_id) = layout_of(&env, &ty);
    assert_eq!(decoded.render(root), original.render(original_id));
    // Only the layouts the value reaches, each after the ones it holds.
    assert!(decoded.ids().all(|id| {
        !matches!(&decoded.get(id).kind, LayoutKind::Struct { class: Some(name), .. } if name == "Unused")
    }));
    assert!(decoded
        .ids()
        .any(|id| matches!(decoded.get(id).kind, LayoutKind::Indirect(_))));
    for id in decoded.ids() {
        let inline = match &decoded.get(id).kind {
            LayoutKind::Struct { fields, .. } => fields.clone(),
            LayoutKind::Tagged { variants, .. } => variants
                .iter()
                .flat_map(|variant| variant.fields.clone())
                .collect(),
            _ => Vec::new(),
        };
        assert!(inline.iter().all(|field| field.layout < id));
    }
    // A type without a layout has no descriptor.
    let view = Type::ReturnedView(Box::new(crate::types::ReturnedViewType {
        mutable: false,
        pointee: named("int64"),
        origin: 0,
    }));
    assert!(encode_descriptor(&view, &env).is_err());
}

#[test]
fn decoding_refuses_malformed_descriptors() {
    let env = LayoutEnv::default();
    let good = encode_descriptor(&named("int64"), &env).expect("a layout");
    // One layout of type `int64`: size, alignment, and then `rest`.
    let one = |rest: &str| {
        let mut out = format!("{LAYOUT_DESCRIPTOR_PREFIX}1;0;");
        type_descriptor::push_type(&mut out, &named("int64"));
        out.push_str(rest);
        out
    };
    assert!(decode_descriptor(&one("8;8;s5:int64")).is_ok());
    for (descriptor, message) in [
        ("aura-layout-v1:u0;0;".to_string(), "unknown version"),
        (format!("{good}x"), "trailing bytes"),
        (good[..good.len() - 2].to_string(), "ends early"),
        (
            format!("{LAYOUT_DESCRIPTOR_PREFIX}1;1;"),
            "names a missing layout",
        ),
        (one("8;8;x"), "unknown layout kind"),
        (one("8;8;s4:int9"), "unknown scalar"),
        (one("4;4;s5:int64"), "wrong scalar size"),
        (one("8;8;i1;"), "names a missing layout"),
        (one("8;8;c2"), "malformed name"),
        (one("8;8;c01;1:x0;0;"), "holds a later layout in place"),
        (one("8;8;t0300;0;0;"), "malformed tag"),
    ] {
        let error = decode_descriptor(&descriptor).unwrap_err();
        assert!(error.contains(message), "{descriptor}: {error}");
    }
}

#[test]
fn strongly_connected_components_split_a_graph() {
    // 0 -> 1 -> 2 -> 0, 2 -> 3, 3 -> 3, 4 alone.
    let edges = vec![vec![1], vec![2], vec![0, 3], vec![3], vec![]];
    let mut components = strongly_connected_components(&edges);
    components.sort();
    assert_eq!(components, vec![vec![0, 1, 2], vec![3], vec![4]]);
}

#[test]
fn an_enum_in_a_box_keeps_scalars_inline_and_every_other_payload_in_a_box() {
    let mut env = LayoutEnv::default();
    env.enums.insert(
        "Tree".to_string(),
        enumeration(&[
            ("Node", vec![named("Tree"), named("Tree")]),
            ("Leaf", vec![named("int32"), named("str")]),
            ("Empty", Vec::new()),
        ]),
    );
    env.enums.insert(
        "Maybe".to_string(),
        EnumDefinition {
            type_params: vec!["T".to_string()],
            variants: vec![("Some".to_string(), vec![Type::TypeParam("T".to_string())])],
        },
    );
    let inline = |ty: &Type| matches!(ty, Type::Named(name, _) if name == "int32");
    let mut builder = LayoutBuilder::new(&env);
    let id = builder
        .boxed_enum_layout(&named("Tree"), inline)
        .expect("a layout")
        .expect("an enum");
    // A later layout of the enum lays its recursive edges out as before.
    let held = builder.layout(&named("Tree")).expect("a layout");
    // Neither a class, an enum that names a type parameter, nor a type
    // that is no enum is laid out in a box.
    for ty in [
        named("int64"),
        named("Missing"),
        generic("Maybe", vec![Type::TypeParam("T".to_string())]),
        Type::Tuple(vec![named("int64")]),
    ] {
        assert_eq!(
            builder.boxed_enum_layout(&ty, inline).unwrap(),
            None,
            "{ty}"
        );
    }
    let concrete = builder
        .boxed_enum_layout(&generic("Maybe", vec![named("int32")]), inline)
        .expect("a layout")
        .expect("an enum");
    let table = builder.into_table();
    table.validate().expect("a valid table");
    assert!(!table.holds_block(id));
    assert!(table.holds_block(held));
    let LayoutKind::Tagged {
        enum_name,
        variants,
        payload_offset,
        ..
    } = &table.get(id).kind
    else {
        panic!("expected a tagged layout");
    };
    assert_eq!(enum_name.as_deref(), Some("Tree"));
    let kinds = |variant: usize| {
        variants[variant]
            .fields
            .iter()
            .map(|field| {
                (
                    field.offset - payload_offset,
                    table.get(field.layout).kind.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(0),
        vec![(0, LayoutKind::Handle), (8, LayoutKind::Handle)]
    );
    assert_eq!(
        kinds(1),
        vec![
            (0, LayoutKind::Scalar(ScalarLayout::Int32)),
            (8, LayoutKind::Handle)
        ]
    );
    assert!(kinds(2).is_empty());
    let LayoutKind::Tagged { variants, .. } = &table.get(concrete).kind else {
        panic!("expected a tagged layout");
    };
    assert_eq!(
        table.get(variants[0].fields[0].layout).kind,
        LayoutKind::Scalar(ScalarLayout::Int32)
    );
    let descriptor = encode_layouts(&table, id);
    let (decoded, root) = decode_descriptor(&descriptor).expect("a descriptor");
    assert!(matches!(decoded.get(root).kind, LayoutKind::Tagged { .. }));
}

#[test]
fn a_class_in_a_box_keeps_the_fields_it_accepts_inline_and_the_rest_in_boxes() {
    let mut env = LayoutEnv::default();
    env.enums.insert(
        "Value".to_string(),
        enumeration(&[("Int", vec![named("int64")]), ("Text", vec![named("str")])]),
    );
    env.classes.insert(
        "Condition".to_string(),
        class(&[
            ("column", named("int64")),
            ("value", named("Value")),
            ("name", named("str")),
            ("flag", named("bool")),
        ]),
    );
    let inline = |ty: &Type| !matches!(ty, Type::Named(name, _) if name == "Value");
    let mut builder = LayoutBuilder::new(&env);
    let id = builder
        .boxed_class_layout(&named("Condition"), inline)
        .expect("a layout")
        .expect("a class");
    // Neither an enum nor a type that is no class is laid out as a class.
    for ty in [named("Value"), named("int64"), named("Missing")] {
        assert_eq!(
            builder.boxed_class_layout(&ty, inline).unwrap(),
            None,
            "{ty}"
        );
    }
    let table = builder.into_table();
    table.validate().expect("a valid table");
    let LayoutKind::Struct { class, fields } = &table.get(id).kind else {
        panic!("expected a struct layout");
    };
    assert_eq!(class.as_deref(), Some("Condition"));
    let laid = fields
        .iter()
        .map(|field| (field.offset, table.get(field.layout).kind.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        laid,
        vec![
            (0, LayoutKind::Scalar(ScalarLayout::Int64)),
            (8, LayoutKind::Handle),
            (16, LayoutKind::Handle),
            (24, LayoutKind::Scalar(ScalarLayout::Bool)),
        ]
    );
    assert_eq!(table.get(id).size, 32);
    let descriptor = encode_layouts(&table, id);
    let (decoded, root) = decode_descriptor(&descriptor).expect("a descriptor");
    assert!(matches!(decoded.get(root).kind, LayoutKind::Struct { .. }));
}
