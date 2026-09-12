//! Forged-MIR regressions for complete callable contracts (Q17 A, Q19 A).
//! A stored contract may hide a name or drop default availability, never
//! rename a slot, invent a default, or expose a keyword-only slot
//! positionally; the shared validator rejects every forged local contract on
//! both public boundaries.

use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

fn encode(source: &str) -> Value {
    serde_json::to_value(lower_source_to_mir(source).expect("source should lower")).unwrap()
}

fn function_mut<'a>(encoded: &'a mut Value, name: &str) -> &'a mut Value {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn assert_rejected(encoded: Value, expected: &str) {
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let interpreted = run_mir(&mir).expect_err("interpreter must reject the forged module");
    assert!(
        interpreted.message.contains(expected),
        "interpreter rejection `{}` should mention `{expected}`",
        interpreted.message
    );
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert!(
        native.contains(expected),
        "native rejection `{native}` should mention `{expected}`"
    );
}

/// The contract slot named `slot` of the first parameter of `function`,
/// which is the written boundary every argument is checked against.
fn boundary_slot_mut<'a>(function: &'a mut Value, slot: &str) -> &'a mut Value {
    function["params"][0]
        .get_mut("ty")
        .and_then(|ty| ty.get_mut("Function"))
        .and_then(|function| function.get_mut("params"))
        .and_then(Value::as_array_mut)
        .expect("the parameter is a written callable contract")
        .iter_mut()
        .find(|param| param["name"] == slot)
        .expect("the boundary should carry the contract slot")
}

const NAMED_SOURCE: &str = "def apply(callback: def(value: int64) -> int64) -> int64:\n    return callback(1)\ndef twice(value: int64) -> int64:\n    return value * 2\ndef main():\n    print(apply(twice))\n";

#[test]
fn forged_boundary_contract_cannot_rename_a_declared_slot() {
    let mut encoded = encode(NAMED_SOURCE);
    let apply = function_mut(&mut encoded, "apply");
    boundary_slot_mut(apply, "value")["name"] = json!("other");
    assert_rejected(encoded, "changes an authoritative callable contract");
}

#[test]
fn forged_boundary_contract_cannot_invent_a_default() {
    let mut encoded = encode(NAMED_SOURCE);
    let apply = function_mut(&mut encoded, "apply");
    boundary_slot_mut(apply, "value")["has_default"] = json!(true);
    assert_rejected(encoded, "changes an authoritative callable contract");
}

#[test]
fn forged_boundary_contract_cannot_expose_a_keyword_only_slot_positionally() {
    let source = "def apply(callback: def(*, value: int64) -> int64) -> int64:\n    return callback(value=1)\ndef keyed(*, value: int64) -> int64:\n    return value\ndef main():\n    print(apply(keyed))\n";
    let mut encoded = encode(source);
    let apply = function_mut(&mut encoded, "apply");
    let slot = boundary_slot_mut(apply, "value");
    assert_eq!(slot["keyword_only"], json!(true));
    slot["keyword_only"] = json!(false);
    assert_rejected(encoded, "changes an authoritative callable contract");
}

#[test]
fn forged_closure_declaration_cannot_change_its_keyword_boundary() {
    let source = "def main():\n    add: def(value: int64) -> int64 = lambda value: value + 1\n    print(add(2))\n";
    let mut encoded = encode(source);
    let closure_name = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|function| function["name"].as_str().unwrap().to_string())
        .find(|name| name != "main")
        .expect("the lambda lowers to a closure function");
    let closure = function_mut(&mut encoded, &closure_name);
    let param = closure["params"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|param| param["name"] == "value")
        .expect("the closure exposes its parameter");
    param["keyword_only"] = json!(true);
    assert_rejected(encoded, "does not match declaration");
}

#[test]
fn a_written_positional_only_contract_still_admits_a_named_target() {
    let source = "def twice(value: int64) -> int64:\n    return value * 2\ndef main():\n    step: def(int64) -> int64 = twice\n    print(step(4))\n";
    let mir = lower_source_to_mir(source).expect("a hidden name is an admitted restriction");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, "8\n");
    emit_host_native_object(&mir).expect("the direct backend accepts the restriction");
}
