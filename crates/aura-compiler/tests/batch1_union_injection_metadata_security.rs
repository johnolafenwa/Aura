//! Malformed serialized MIR union-injection metadata must fail validation.

use aura_compiler::{lower_source_to_mir, run_serialized_mir};
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

fn assert_rejected(mut value: Value, mutate: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    mutate(injection_payload(&mut value));
    let bytes = serde_json::to_vec(&value).expect("forged MIR JSON must serialize");
    let error = run_serialized_mir(&bytes, "<forged-union-injection>", "")
        .expect_err("validation must reject forged union injection metadata before execution");
    assert!(error.message.contains("union injection"), "{error}");
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
    assert_rejected(valid_serialized_injection(), |payload| {
        payload.insert("member_index".to_string(), Value::from(usize::MAX));
    });
}

#[test]
fn serialized_union_injection_rejects_mismatched_member_type() {
    assert_rejected(valid_serialized_injection(), |payload| {
        let union = payload["union_type"].clone();
        payload.insert("member_type".to_string(), union);
    });
}

#[test]
fn serialized_union_injection_rejects_scalar_union_type() {
    assert_rejected(valid_serialized_injection(), |payload| {
        let member = payload["member_type"].clone();
        payload.insert("union_type".to_string(), member);
    });
}

#[test]
fn serialized_union_injection_rejects_operand_static_type_mismatch() {
    assert_rejected(valid_serialized_injection(), |payload| {
        payload.insert(
            "value".to_string(),
            serde_json::json!({ "String": "forged" }),
        );
    });
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

    let bytes = serde_json::to_vec(&value).expect("forged MIR JSON must serialize");
    let error = run_serialized_mir(&bytes, "<forged-union-injection>", "")
        .expect_err("validation must reject a scalar destination before execution");
    assert!(error.message.contains("union injection"), "{error}");
}

#[test]
fn serialized_union_injection_rejects_unknown_payload_place() {
    let mut value = valid_serialized_injection();
    injection_payload(&mut value).insert(
        "value".to_string(),
        serde_json::json!({ "Place": "missing_payload" }),
    );

    let bytes = serde_json::to_vec(&value).expect("forged MIR JSON must serialize");
    let error = run_serialized_mir(&bytes, "<forged-union-injection>", "")
        .expect_err("validation must reject an unknown payload place before execution");
    assert!(
        error.message.contains("union injection operand")
            || error.message.contains("missing_payload")
            || error.message.contains("unknown"),
        "{error}"
    );
}
