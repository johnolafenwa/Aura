//! Malformed serialized MIR union-injection metadata must fail validation on
//! every public boundary: the serialized-module entry point, the interpreter,
//! and the direct backend must all refuse the same corruption for the same
//! shared validator reason before anything executes.

use aura_compiler::{
    emit_host_native_object, lower_source_to_mir, run_mir, run_serialized_mir, MirModule,
};
use serde_json::Value;

fn find_injection_payload(value: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => {
            if object.contains_key("member_index")
                && object.contains_key("member_type")
                && object.contains_key("union_type")
                && object.contains_key("value")
            {
                return Some(object);
            }
            for child in object.values_mut() {
                if let Some(found) = find_injection_payload(child) {
                    return Some(found);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                if let Some(found) = find_injection_payload(child) {
                    return Some(found);
                }
            }
        }
        _ => {}
    }
    None
}

fn injection_payload(value: &mut Value) -> &mut serde_json::Map<String, Value> {
    find_injection_payload(value).expect("lowered MIR must contain a union injection")
}

fn valid_serialized_injection() -> Value {
    let mir = lower_source_to_mir("def main():\n    value: int64 | str = 42\n")
        .expect("source union injection must lower");
    serde_json::to_value(mir).expect("MIR must serialize")
}

/// The serialized entry point, the interpreter, and the direct backend must
/// all refuse the forged module, and the two in-memory boundaries must report
/// the identical shared validator reason (the interpreter prefixes it with
/// `invalid MIR loan flow: `). Returns the shared message.
fn assert_rejected_everywhere(value: Value, expected: &str) -> String {
    let bytes = serde_json::to_vec(&value).expect("forged MIR JSON must serialize");
    let serialized = run_serialized_mir(&bytes, "<forged-union-injection>", "")
        .expect_err("the serialized boundary must reject forged union injection metadata");
    let mir: MirModule = serde_json::from_value(value).expect("forged MIR must deserialize");
    let interpreted = run_mir(&mir).expect_err("the interpreter must reject the forged module");
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert_eq!(
        interpreted.message.strip_prefix("invalid MIR loan flow: "),
        Some(native.as_str()),
        "both in-memory boundaries must report the same shared validator reason"
    );
    assert!(
        serialized.message.ends_with(&native),
        "serialized rejection `{}` should end with the shared validator reason `{native}`",
        serialized.message
    );
    assert!(
        native.contains(expected),
        "shared rejection `{native}` should mention `{expected}`"
    );
    native
}

fn assert_rejected(
    mut value: Value,
    mutate: impl FnOnce(&mut serde_json::Map<String, Value>),
    expected: &str,
) -> String {
    mutate(injection_payload(&mut value));
    assert_rejected_everywhere(value, expected)
}

fn find_injection_assignment_target(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => {
            if let Some(Value::Object(assign)) = object.get("Assign") {
                if assign.get("value").is_some_and(|value| {
                    let mut copy = value.clone();
                    find_injection_payload(&mut copy).is_some()
                }) {
                    return assign
                        .get("target")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            object.values().find_map(find_injection_assignment_target)
        }
        Value::Array(items) => items.iter().find_map(find_injection_assignment_target),
        _ => None,
    }
}

fn forge_local_type(value: &mut Value, name: &str, replacement: &Value) -> bool {
    match value {
        Value::Object(object) => {
            if object.get("name").and_then(Value::as_str) == Some(name) && object.contains_key("ty")
            {
                object.insert("ty".to_string(), replacement.clone());
                return true;
            }
            object
                .values_mut()
                .any(|child| forge_local_type(child, name, replacement))
        }
        Value::Array(items) => items
            .iter_mut()
            .any(|child| forge_local_type(child, name, replacement)),
        _ => false,
    }
}

#[test]
fn serialized_union_injection_rejects_out_of_range_member_index() {
    assert_rejected(
        valid_serialized_injection(),
        |payload| {
            payload.insert("member_index".to_string(), Value::from(usize::MAX));
        },
        "union injection member index and type disagree",
    );
}

#[test]
fn serialized_union_injection_rejects_mismatched_member_type() {
    assert_rejected(
        valid_serialized_injection(),
        |payload| {
            let union = payload["union_type"].clone();
            payload.insert("member_type".to_string(), union);
        },
        "union injection member index and type disagree",
    );
}

#[test]
fn serialized_union_injection_rejects_scalar_union_type() {
    assert_rejected(
        valid_serialized_injection(),
        |payload| {
            let member = payload["member_type"].clone();
            payload.insert("union_type".to_string(), member);
        },
        "union injection requires a union type",
    );
}

#[test]
fn serialized_union_injection_rejects_operand_static_type_mismatch() {
    assert_rejected(
        valid_serialized_injection(),
        |payload| {
            payload.insert(
                "value".to_string(),
                serde_json::json!({ "String": "forged" }),
            );
        },
        "union injection operand does not have its selected member type",
    );
}

#[test]
fn serialized_union_injection_rejects_scalar_assignment_destination() {
    let mut value = valid_serialized_injection();
    let target = find_injection_assignment_target(&value)
        .expect("lowered injection must be assigned to a typed destination");
    let scalar = injection_payload(&mut value)["member_type"].clone();
    assert!(
        forge_local_type(&mut value, &target, &scalar),
        "assignment target must have local type metadata"
    );
    assert_rejected_everywhere(
        value,
        "union injection destination does not have its union type",
    );
}

#[test]
fn serialized_union_injection_rejects_unknown_payload_place() {
    let mut value = valid_serialized_injection();
    injection_payload(&mut value).insert(
        "value".to_string(),
        serde_json::json!({ "Place": "missing_payload" }),
    );
    // An unknown place has no static type, so the validator reports the
    // operand/member type disagreement rather than resolving the place.
    assert_rejected_everywhere(
        value,
        "union injection operand does not have its selected member type",
    );
}
