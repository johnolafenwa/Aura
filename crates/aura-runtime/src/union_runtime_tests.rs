use super::*;
use crate::integer::IntegerValue;
use crate::runtime_value::{InstanceValue, UnionValue};
use std::collections::BTreeMap;

fn union(members: Vec<Type>) -> UnionType {
    match Type::normalize_union(members, "main", &BTreeMap::new()).expect("normalized union") {
        Type::Union(union) => *union,
        other => panic!("expected a union, found {other}"),
    }
}

fn index_of(target: &UnionType, member: &Type) -> usize {
    target
        .members
        .iter()
        .position(|candidate| candidate == member)
        .expect("member present")
}

fn union_value(target: &UnionType, member: &Type, payload: Value) -> Value {
    Value::Union(Box::new(UnionValue {
        union_type: Type::Union(Box::new(target.clone())).into(),
        member_index: index_of(target, member),
        payload,
    }))
}

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

#[test]
fn exact_union_values_keep_their_index() {
    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    let value = union_value(&concrete, &Type::named("int64"), int(4));
    assert_eq!(
        plan_union_alignment(&value, &concrete, "test"),
        Ok(UnionAlignment::Exact)
    );
    assert_eq!(
        aligned_member_index(&value, &concrete, "test"),
        Ok(index_of(&concrete, &Type::named("int64")))
    );
}

#[test]
fn symbolic_values_retag_to_the_concrete_union_by_payload_type() {
    let symbolic = union(vec![Type::TypeParam("V".to_string()), Type::Unit]);
    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    let mut value = union_value(&symbolic, &Type::TypeParam("V".to_string()), int(4));
    let expected = index_of(&concrete, &Type::named("int64"));
    assert_eq!(
        plan_union_alignment(&value, &concrete, "test"),
        Ok(UnionAlignment::Retag(expected))
    );
    assert_eq!(
        align_union_value(&mut value, &concrete, "test"),
        Ok(expected)
    );
    let Value::Union(aligned) = &value else {
        panic!("aligned value stays a union");
    };
    assert_eq!(*aligned.union_type, Type::Union(Box::new(concrete.clone())));
    assert_eq!(aligned.member_index, expected);
    assert_eq!(aligned.payload, int(4));
}

#[test]
fn concrete_values_retag_to_the_symbolic_parameter_member() {
    let symbolic = union(vec![
        Type::named("str"),
        Type::TypeParam("V".to_string()),
        Type::Unit,
    ]);
    let concrete = union(vec![Type::named("Dog"), Type::named("str"), Type::Unit]);
    let dog = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("Dog".to_string()),
        fields: Default::default(),
    });
    let value = union_value(&concrete, &Type::named("Dog"), dog);
    assert_eq!(
        aligned_member_index(&value, &symbolic, "test"),
        Ok(index_of(&symbolic, &Type::TypeParam("V".to_string())))
    );
    let text = union_value(
        &concrete,
        &Type::named("str"),
        Value::String("x".to_string().into()),
    );
    assert_eq!(
        aligned_member_index(&text, &symbolic, "test"),
        Ok(index_of(&symbolic, &Type::named("str")))
    );
}

#[test]
fn bare_member_values_lift_and_nonmembers_fail_closed() {
    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    assert_eq!(
        plan_union_alignment(&int(4), &concrete, "union tag test"),
        Ok(UnionAlignment::Lift(index_of(
            &concrete,
            &Type::named("int64")
        )))
    );
    assert_eq!(
        plan_union_alignment(&Value::Unit, &concrete, "union tag test"),
        Ok(UnionAlignment::Lift(index_of(&concrete, &Type::Unit)))
    );
    assert_eq!(
        plan_union_alignment(
            &Value::String("x".to_string().into()),
            &concrete,
            "union tag test"
        ),
        Err("union tag test type identity mismatch".to_string())
    );
    let mut lifted = int(4);
    align_union_value(&mut lifted, &concrete, "test").expect("lift");
    assert!(matches!(&lifted, Value::Union(union) if union.payload == int(4)));
}

#[test]
fn injecting_a_union_payload_behind_a_parameter_flattens() {
    let symbolic = union(vec![Type::TypeParam("V".to_string()), Type::Unit]);
    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    let payload = union_value(&concrete, &Type::named("int64"), int(4));
    let parameter = index_of(&symbolic, &Type::TypeParam("V".to_string()));
    let injected = inject_union_member(&symbolic, parameter, payload).expect("flatten");
    let Value::Union(injected) = injected else {
        panic!("injection builds a union");
    };
    assert_eq!(
        *injected.union_type,
        Type::Union(Box::new(symbolic.clone()))
    );
    assert_eq!(injected.member_index, parameter);
    assert_eq!(injected.payload, int(4), "no nested union wrapper");

    let none_payload = union_value(&concrete, &Type::Unit, Value::Unit);
    let flattened = inject_union_member(&symbolic, parameter, none_payload).expect("flatten none");
    let Value::Union(flattened) = flattened else {
        panic!("injection builds a union");
    };
    assert_eq!(flattened.member_index, index_of(&symbolic, &Type::Unit));

    let nested = union_value(&concrete, &Type::named("int64"), int(1));
    assert_eq!(
        inject_union_member(&symbolic, index_of(&symbolic, &Type::Unit), nested),
        Err("union injection cannot nest a union inside a concrete member".to_string())
    );
    assert_eq!(
        inject_union_member(&symbolic, 9, int(1)),
        Err("union injection member index is out of range".to_string())
    );
}

#[test]
fn boundaries_retag_lift_and_unwrap() {
    let symbolic = union(vec![Type::TypeParam("V".to_string()), Type::Unit]);
    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    let concrete_type = Type::Union(Box::new(concrete.clone()));

    let value = union_value(&symbolic, &Type::TypeParam("V".to_string()), int(4));
    let Value::Union(retagged) = coerce_union_boundary(value, &concrete_type) else {
        panic!("a union stays a union at a union boundary");
    };
    assert_eq!(*retagged.union_type, concrete_type);

    let Value::Union(lifted) = coerce_union_boundary(int(4), &concrete_type) else {
        panic!("a bare member lifts into an expected union");
    };
    assert_eq!(lifted.payload, int(4));

    let none = union_value(&symbolic, &Type::Unit, Value::Unit);
    assert_eq!(coerce_union_boundary(none, &Type::Unit), Value::Unit);

    let wrong = union_value(&concrete, &Type::named("int64"), int(4));
    assert!(matches!(
        coerce_union_boundary(wrong, &Type::named("str")),
        Value::Union(_)
    ));
    assert_eq!(
        coerce_union_boundary(Value::String("x".to_string().into()), &concrete_type),
        Value::String("x".to_string().into())
    );
    assert_eq!(
        coerce_union_boundary(int(4), &Type::TypeParam("V".to_string())),
        int(4)
    );
}

#[test]
fn member_identity_tolerates_one_sided_module_qualification() {
    assert!(member_matches(
        &Type::named("Dog"),
        &Type::named("main.Dog")
    ));
    assert!(member_matches(
        &Type::named("pets.Dog"),
        &Type::named("Dog")
    ));
    assert!(!member_matches(
        &Type::named("pets.Dog"),
        &Type::named("main.Dog")
    ));
    assert!(!member_matches(
        &Type::named("Cat"),
        &Type::named("main.Dog")
    ));
    assert!(!member_matches(
        &Type::Named("Box".to_string(), vec![Type::named("int64")]),
        &Type::Named("Box".to_string(), vec![Type::named("str")])
    ));
}

#[test]
fn none_tests_see_through_union_tags() {
    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    assert!(is_none_value(&Value::Unit));
    assert!(is_none_value(&union_value(
        &concrete,
        &Type::Unit,
        Value::Unit
    )));
    assert!(!is_none_value(&union_value(
        &concrete,
        &Type::named("int64"),
        int(1)
    )));
    assert!(!is_none_value(&int(1)));
    let symbolic = union(vec![Type::TypeParam("V".to_string()), Type::Unit]);
    assert!(is_none_value(&union_value(
        &symbolic,
        &Type::TypeParam("V".to_string()),
        Value::Unit
    )));
}

#[test]
fn union_equality_is_active_member_equality() {
    let concrete = union(vec![Type::named("int64"), Type::named("str"), Type::Unit]);
    let symbolic = union(vec![Type::TypeParam("V".to_string()), Type::Unit]);
    let one = union_value(&concrete, &Type::named("int64"), int(1));
    let another_one = union_value(&concrete, &Type::named("int64"), int(1));
    let two = union_value(&concrete, &Type::named("int64"), int(2));
    let text = union_value(
        &concrete,
        &Type::named("str"),
        Value::String("1".to_string().into()),
    );
    let none = union_value(&concrete, &Type::Unit, Value::Unit);
    assert!(union_values_equal(&one, &another_one));
    assert!(!union_values_equal(&one, &two));
    assert!(
        !union_values_equal(&one, &text),
        "different tags stay unequal"
    );
    assert!(
        union_values_equal(&one, &int(1)),
        "a bare member compares by injection"
    );
    assert!(union_values_equal(&int(1), &one));
    assert!(!union_values_equal(&text, &int(1)));
    assert!(union_values_equal(&none, &Value::Unit));
    assert!(!union_values_equal(&none, &one));
    let symbolic_one = union_value(&symbolic, &Type::TypeParam("V".to_string()), int(1));
    assert!(
        union_values_equal(&symbolic_one, &one),
        "a generic frame's layout compares by the active member"
    );
    assert_eq!(one, another_one, "the shared PartialEq routes unions here");
    assert_ne!(one, text);
}

#[test]
fn runtime_member_type_identifies_every_backend_independent_payload_kind() {
    use crate::runtime_value::{
        ArrayStorage, ArrayValue, EnumVariantValue, FfiHandleValue, FunctionValue, MapValue,
        ModuleNamespaceValue, RangeValue, RngValue, SetValue, TupleValue, VecValue,
    };

    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    let nested = union_value(&concrete, &Type::named("int64"), int(3));
    assert_eq!(runtime_member_type(&nested), Some(Type::named("int64")));
    assert_eq!(
        runtime_member_type(&Value::Float(1.5)),
        Some(Type::named("float64"))
    );
    assert_eq!(
        runtime_member_type(&Value::Bool(true)),
        Some(Type::named("bool"))
    );
    assert_eq!(
        runtime_member_type(&Value::Tuple(TupleValue {
            element_types: vec![Type::named("int64"), Type::named("str")],
            elements: vec![int(1), Value::String("a".to_string().into())],
        })),
        Some(Type::Tuple(vec![Type::named("int64"), Type::named("str")]))
    );
    assert_eq!(
        runtime_member_type(&Value::Vec(VecValue {
            element_type: crate::runtime_value::SharedType::from(Type::named("str")),
            elements: Vec::new().into(),
            str_view: Default::default(),
        })),
        Some(Type::Named("list".to_string(), vec![Type::named("str")]))
    );
    assert_eq!(
        runtime_member_type(&Value::Array(
            ArrayValue::new(Box::new([2]), ArrayStorage::Int64([1, 2].into())).unwrap()
        )),
        Some(Type::Named("Array".to_string(), vec![Type::named("int64")]))
    );
    assert_eq!(
        runtime_member_type(&Value::Set(SetValue {
            element_type: crate::runtime_value::SharedType::from(Type::named("int64")),
            elements: Vec::new().into(),
        })),
        Some(Type::Named("set".to_string(), vec![Type::named("int64")]))
    );
    assert_eq!(
        runtime_member_type(&Value::Map(MapValue {
            key_type: Type::named("str").into(),
            value_type: Type::named("bool").into(),
            entries: Vec::new().into(),
        })),
        Some(Type::Named(
            "dict".to_string(),
            vec![Type::named("str"), Type::named("bool")]
        ))
    );
    assert_eq!(
        runtime_member_type(&Value::Duration(7)),
        Some(Type::named("Duration"))
    );
    assert_eq!(
        runtime_member_type(&Value::Rng(RngValue::from_seed(1))),
        Some(Type::named("random.Rng"))
    );
    assert_eq!(
        runtime_member_type(&Value::Range(RangeValue {
            start: 0,
            end: 2,
            step: 1
        })),
        Some(Type::named("Range"))
    );
    let signature = Type::Function {
        params: Vec::new(),
        return_type: Box::new(Type::Unit),
    };
    assert_eq!(
        runtime_member_type(&Value::Function(Arc::new(FunctionValue {
            name: "f".to_string(),
            signature: signature.clone(),
            source_path: None,
            entry_span: crate::diag::Span::new(1, 1),
            direct_thunk: None,
            direct_default_binder: None,
            closure_environment: None,
            direct_callable: None,
        }))),
        Some(signature)
    );
    assert_eq!(runtime_member_type(&Value::Unit), Some(Type::Unit));
    let mut backing = 0u8;
    let handle = FfiHandleValue::new(
        "Handle".to_string(),
        (&mut backing as *mut u8).cast::<std::ffi::c_void>(),
    )
    .expect("non-null handle");
    assert_eq!(
        runtime_member_type(&Value::FfiHandle(handle)),
        Some(Type::named("Handle"))
    );
    assert_eq!(
        runtime_member_type(&Value::Instance(InstanceValue {
            class_name: crate::runtime_value::RuntimeName::from("Dog".to_string()),
            fields: Default::default(),
        })),
        Some(Type::named("Dog"))
    );
    assert_eq!(
        runtime_member_type(&Value::EnumVariant(EnumVariantValue {
            enum_name: "Shape".into(),
            variant_name: "Circle".into(),
            payloads: Vec::new().into(),
        })),
        Some(Type::named("Shape"))
    );
    assert_eq!(
        runtime_member_type(&Value::ModuleNamespace(ModuleNamespaceValue {
            path: "m".to_string(),
        })),
        None
    );
}

#[test]
fn values_without_a_member_identity_never_compare_equal_or_align() {
    use crate::runtime_value::ModuleNamespaceValue;

    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    let namespace = Value::ModuleNamespace(ModuleNamespaceValue {
        path: "m".to_string(),
    });
    let one = union_value(&concrete, &Type::named("int64"), int(1));
    assert!(!union_values_equal(&namespace, &one));
    assert!(!union_values_equal(&one, &namespace));
    assert_eq!(
        plan_union_alignment(&namespace, &concrete, "union test"),
        Err("union test cannot identify the active union member".to_string())
    );
    assert_eq!(
        aligned_member_index(&namespace, &concrete, "union take"),
        Err("union take cannot identify the active union member".to_string())
    );
    assert!(!is_none_value(&namespace));
}

#[test]
fn forged_union_values_without_a_union_type_have_no_identity() {
    let forged = Value::Union(Box::new(UnionValue {
        union_type: Type::named("int64").into(),
        member_index: 0,
        payload: int(1),
    }));
    assert_eq!(member_identity(&forged), None);
    assert_eq!(runtime_member_type(&forged), None);
    let concrete = union(vec![Type::named("int64"), Type::Unit]);
    assert_eq!(
        plan_union_alignment(&forged, &concrete, "union tag test"),
        Err("union tag test cannot identify the active union member".to_string())
    );
    assert!(!union_values_equal(&forged, &int(1)));
}

#[test]
fn cross_module_nominal_collision_distinguishes_imported_and_entry_classes() {
    // Entry module `main` defines `Item` while importing `alpha.Item`.
    // Canonical sorting orders `"alpha.Item"` (index 0) before `"main.Item"` (index 1).
    let choice = union(vec![Type::named("alpha.Item"), Type::named("Item")]);
    let remote_index = index_of(&choice, &Type::named("alpha.Item"));
    let local_index = index_of(&choice, &Type::named("Item"));
    assert_eq!((remote_index, local_index), (0, 1));

    let remote_instance = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("alpha.Item".to_string()),
        fields: Default::default(),
    });
    let local_instance = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("Item".to_string()),
        fields: Default::default(),
    });

    assert_eq!(
        plan_union_alignment(&remote_instance, &choice, "lift remote"),
        Ok(UnionAlignment::Lift(remote_index))
    );
    assert_eq!(
        plan_union_alignment(&local_instance, &choice, "lift local"),
        Ok(UnionAlignment::Lift(local_index))
    );

    let remote_union = union_value(&choice, &Type::named("alpha.Item"), remote_instance.clone());
    let local_union = union_value(&choice, &Type::named("Item"), local_instance.clone());
    assert!(!union_values_equal(&remote_union, &local_union));
    assert!(!union_values_equal(&remote_union, &local_instance));
    assert!(union_values_equal(&local_union, &local_instance));

    // Ambiguous bare `Item` against two distinct imported `alpha.Item | beta.Item` must fail closed.
    let two_imported = union(vec![Type::named("alpha.Item"), Type::named("beta.Item")]);
    assert!(plan_union_alignment(&local_instance, &two_imported, "ambiguous").is_err());
}

#[test]
fn erased_trait_object_union_members_keep_distinct_tags() {
    let dyn_a = Type::TypeParam("dyn A".to_string());
    let dyn_b = Type::TypeParam("dyn B".to_string());
    let concrete = Type::named("Concrete");
    let only_b = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("OnlyB".to_string()),
        fields: Default::default(),
    });
    let both = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("Both".to_string()),
        fields: Default::default(),
    });
    let concrete_val = Value::Instance(InstanceValue {
        class_name: crate::runtime_value::RuntimeName::from("Concrete".to_string()),
        fields: Default::default(),
    });

    // Widening `dyn B | None` to `dyn A | dyn B | None` must keep the `dyn B` tag.
    let b_opt = union(vec![dyn_b.clone(), Type::Unit]);
    let ab_opt = union(vec![dyn_a.clone(), dyn_b.clone(), Type::Unit]);
    let b_val = union_value(&b_opt, &dyn_b, only_b.clone());
    assert_eq!(
        plan_union_alignment(&b_val, &ab_opt, "widen"),
        Ok(UnionAlignment::Retag(index_of(&ab_opt, &dyn_b)))
    );

    // Aligning `dyn B | None` into `dyn A | None` must fail closed.
    let a_opt = union(vec![dyn_a.clone(), Type::Unit]);
    assert_eq!(
        plan_union_alignment(&b_val, &a_opt, "mismatch"),
        Err("mismatch type identity mismatch".to_string())
    );

    // `dyn A | Concrete` holding `dyn A(Concrete)` must not downcast to `Concrete`.
    let a_or_concrete = union(vec![dyn_a.clone(), concrete.clone()]);
    let as_dyn_a = union_value(
        &a_or_concrete,
        &dyn_a,
        Value::TraitObject(Box::new(crate::runtime_value::TraitObjectValue {
            held: Arc::new(concrete.clone()),
            value: concrete_val.clone(),
        })),
    );
    assert_eq!(member_identity(&as_dyn_a), Some(dyn_a.clone()));
    assert!(matches!(
        coerce_union_boundary(as_dyn_a.clone(), &concrete),
        Value::Union(_)
    ));
    let as_concrete = union_value(&a_or_concrete, &concrete, concrete_val.clone());
    assert!(!union_values_equal(&as_dyn_a, &as_concrete));
    assert!(!union_values_equal(&as_dyn_a, &concrete_val));

    // Distinct trait object tags with identical payloads are unequal.
    let ab = union(vec![dyn_a.clone(), dyn_b.clone()]);
    let both_as_a = union_value(&ab, &dyn_a, both.clone());
    let both_as_b = union_value(&ab, &dyn_b, both);
    assert!(!union_values_equal(&both_as_a, &both_as_b));
}

#[test]
fn untagged_trait_object_payloads_never_downcast_to_a_concrete_member() {
    let dyn_a = Type::TypeParam("dyn A".to_string());
    let dyn_b = Type::TypeParam("dyn B".to_string());
    let param = Type::TypeParam("T".to_string());
    let concrete = Type::named("Concrete");
    let object = Value::TraitObject(Box::new(crate::runtime_value::TraitObjectValue {
        held: Arc::new(concrete.clone()),
        value: Value::Instance(InstanceValue {
            class_name: crate::runtime_value::RuntimeName::from("Concrete".to_string()),
            fields: Default::default(),
        }),
    }));

    // A single erased trait-object member takes the payload, not `Concrete`.
    let with_dyn = union(vec![concrete.clone(), dyn_a.clone(), Type::Unit]);
    assert_eq!(
        plan_union_alignment(&object, &with_dyn, "lift"),
        Ok(UnionAlignment::Lift(index_of(&with_dyn, &dyn_a)))
    );
    let lifted = coerce_union_boundary(object.clone(), &Type::Union(Box::new(with_dyn.clone())));
    let Value::Union(lifted) = lifted else {
        panic!("a trait object lifts into the union");
    };
    assert_eq!(lifted.member_index, index_of(&with_dyn, &dyn_a));

    // A wildcard type parameter is preferred over any trait-object member.
    let with_param = union(vec![concrete.clone(), param.clone(), dyn_a.clone()]);
    assert_eq!(
        plan_union_alignment(&object, &with_param, "lift"),
        Ok(UnionAlignment::Lift(index_of(&with_param, &param)))
    );

    // Two trait-object members are ambiguous, and a concrete-only union has
    // no admissible member: both fail closed.
    let ambiguous = union(vec![concrete.clone(), dyn_a.clone(), dyn_b]);
    assert_eq!(
        plan_union_alignment(&object, &ambiguous, "pick"),
        Err("pick type identity mismatch".to_string())
    );
    let concrete_only = union(vec![concrete.clone(), Type::Unit]);
    assert_eq!(
        plan_union_alignment(&object, &concrete_only, "pick"),
        Err("pick type identity mismatch".to_string())
    );

    // A symbolic union holding a trait object stays wrapped at a concrete
    // boundary instead of unwrapping to the held class.
    let symbolic = union(vec![param.clone(), Type::Unit]);
    let held = union_value(&symbolic, &param, object);
    assert!(matches!(
        coerce_union_boundary(held, &concrete),
        Value::Union(_)
    ));
}
