use super::*;
use crate::integer::IntegerValue;
use crate::runtime_value::InstanceValue;
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
        union_type: Type::Union(Box::new(target.clone())),
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
    assert_eq!(aligned.union_type, Type::Union(Box::new(concrete.clone())));
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
        class_name: "Dog".to_string(),
        fields: BTreeMap::new(),
    });
    let value = union_value(&concrete, &Type::named("Dog"), dog);
    assert_eq!(
        aligned_member_index(&value, &symbolic, "test"),
        Ok(index_of(&symbolic, &Type::TypeParam("V".to_string())))
    );
    let text = union_value(
        &concrete,
        &Type::named("str"),
        Value::String("x".to_string()),
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
        plan_union_alignment(&Value::String("x".to_string()), &concrete, "union tag test"),
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
    assert_eq!(injected.union_type, Type::Union(Box::new(symbolic.clone())));
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
    assert_eq!(retagged.union_type, concrete_type);

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
        coerce_union_boundary(Value::String("x".to_string()), &concrete_type),
        Value::String("x".to_string())
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
        Value::String("1".to_string()),
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
