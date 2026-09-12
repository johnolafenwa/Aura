//! Adversarial wire-format contracts for normalized union metadata.

use aura_compiler::{check_source, sema::Type};
use serde_json::{json, Value};

fn parameter(source: &str, function: &str) -> Type {
    check_source(source)
        .expect("control source must check")
        .functions[function]
        .signature
        .params[0]
        .clone()
}

fn scalar_union() -> Type {
    parameter(
        "def accept(value: int64 | str | None):\n    pass\n",
        "accept",
    )
}

fn union_body(value: &mut Value) -> &mut serde_json::Map<String, Value> {
    value
        .get_mut("Union")
        .and_then(Value::as_object_mut)
        .expect("the checked control type must serialize as a Union")
}

fn assert_rejected(value: Value) {
    let first = serde_json::from_value::<Type>(value.clone())
        .expect_err("noncanonical union metadata must be rejected");
    let second = serde_json::from_value::<Type>(value)
        .expect_err("the same noncanonical metadata must be rejected repeatedly");
    assert_eq!(
        first.to_string(),
        second.to_string(),
        "wire validation failures must be deterministic"
    );
    assert!(
        first.to_string().contains("union"),
        "the rejection must identify union metadata: {first}"
    );
}

#[test]
fn valid_checked_union_roundtrips_without_changing_identity() {
    let checked = scalar_union();
    let encoded = serde_json::to_value(&checked).unwrap();
    let decoded: Type = serde_json::from_value(encoded).expect("canonical metadata must roundtrip");
    assert_eq!(decoded, checked);
    assert_eq!(
        serde_json::to_string(&decoded).unwrap(),
        serde_json::to_string(&checked).unwrap()
    );
}

#[test]
fn qualified_nominal_member_keys_survive_interface_roundtrip() {
    let encoded = json!({
        "Union": {
            "members": [
                {"Named": ["tools.Alpha", []]},
                {"Named": ["tools.Beta", []]}
            ],
            "keys": [
                "aura-type-key-v1:[\"named\",\"tools.Alpha\",[]]",
                "aura-type-key-v1:[\"named\",\"tools.Beta\",[]]"
            ],
            "module_name": "consumer"
        }
    });

    let decoded: Type =
        serde_json::from_value(encoded.clone()).expect("qualified identities are self-contained");
    assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
}

#[test]
fn empty_union_metadata_is_rejected() {
    let mut encoded = serde_json::to_value(scalar_union()).unwrap();
    let union = union_body(&mut encoded);
    union.insert("members".to_string(), Value::Array(Vec::new()));
    union.insert("keys".to_string(), Value::Array(Vec::new()));
    assert_rejected(encoded);
}

#[test]
fn singleton_union_metadata_is_rejected_instead_of_bypassing_collapse() {
    let mut encoded = serde_json::to_value(scalar_union()).unwrap();
    let union = union_body(&mut encoded);
    union["members"].as_array_mut().unwrap().truncate(1);
    union["keys"].as_array_mut().unwrap().truncate(1);
    assert_rejected(encoded);
}

#[test]
fn mismatched_key_and_member_lengths_are_rejected() {
    let mut encoded = serde_json::to_value(scalar_union()).unwrap();
    union_body(&mut encoded)["keys"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_rejected(encoded);
}

#[test]
fn duplicate_union_keys_are_rejected() {
    let mut encoded = serde_json::to_value(scalar_union()).unwrap();
    let keys = union_body(&mut encoded)["keys"].as_array_mut().unwrap();
    keys[1] = keys[0].clone();
    assert_rejected(encoded);
}

#[test]
fn unsorted_union_keys_and_members_are_rejected() {
    let mut encoded = serde_json::to_value(scalar_union()).unwrap();
    let union = union_body(&mut encoded);
    union["keys"].as_array_mut().unwrap().reverse();
    union["members"].as_array_mut().unwrap().reverse();
    assert_rejected(encoded);
}

#[test]
fn nested_unflattened_union_metadata_is_rejected() {
    let checked = scalar_union();
    let nested = serde_json::to_value(&checked).unwrap();
    let mut encoded = nested.clone();
    union_body(&mut encoded)["members"].as_array_mut().unwrap()[0] = nested;
    assert_rejected(encoded);
}

#[test]
fn forged_key_that_does_not_describe_its_member_is_rejected() {
    let mut encoded = serde_json::to_value(scalar_union()).unwrap();
    union_body(&mut encoded)["keys"].as_array_mut().unwrap()[0] =
        Value::String("aura-type-key-v1:[\"named\",\"forged.Type\",[]]".to_string());
    assert_rejected(encoded);
}

#[test]
fn forged_defining_module_identity_is_rejected() {
    let checked = parameter(
        "class Alpha:\n    pass\nclass Beta:\n    pass\ndef accept(value: Alpha | Beta):\n    pass\n",
        "accept",
    );
    let mut encoded = serde_json::to_value(checked).unwrap();
    union_body(&mut encoded).insert(
        "module_name".to_string(),
        Value::String("forged.module".to_string()),
    );
    assert_rejected(encoded);
}
