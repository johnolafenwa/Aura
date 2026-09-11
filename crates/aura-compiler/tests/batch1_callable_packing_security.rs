//! Forged-MIR regressions for owned callable storage (Q13-Q16 A, Q21 A). A
//! packed value keeps its closure's own environment; only its static type is
//! erased. The shared validator must therefore refuse a forged erased
//! contract that strengthens the value's call kind, hides a loan capture, or
//! declares an environment-owned mutable capture over a borrowed source.

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

/// The erased callable contract of the first parameter of `function`.
fn boundary_callable_mut<'a>(function: &'a mut Value) -> &'a mut Value {
    function["params"][0]
        .get_mut("ty")
        .and_then(|ty| ty.get_mut("Callable"))
        .expect("the parameter is an erased callable contract")
}

const MUTABLE_SOURCE: &str = "class Counter:\n    total: int64\n\n    def bump(mut self) -> int64:\n        self.total = self.total + 1\n        return self.total\n\ntype Bumper = Callable[mut def() -> int64]\n\ndef bump_through(bumper: mut Bumper) -> int64:\n    return bumper()\n\ndef main():\n    counter = Counter(total=0)\n    mut bumper = Bumper(lambda [own counter]: counter.bump())\n    print(bump_through(bumper))\n";

#[test]
fn forged_boundary_cannot_weaken_a_mutable_closure_to_shared_storage() {
    let mut encoded = encode(MUTABLE_SOURCE);
    let callee = function_mut(&mut encoded, "bump_through");
    let contract = boundary_callable_mut(callee);
    assert_eq!(contract["call_kind"], json!("MutableRepeatable"));
    contract["call_kind"] = json!("Repeatable");
    assert_rejected(encoded, "changes an authoritative callable contract");
}

#[test]
fn forged_boundary_cannot_change_the_erased_contract_result() {
    let mut encoded = encode(MUTABLE_SOURCE);
    let callee = function_mut(&mut encoded, "bump_through");
    let contract = boundary_callable_mut(callee);
    assert_eq!(contract["return_type"], json!({"Named": ["int64", []]}));
    contract["return_type"] = json!({"Named": ["str", []]});
    assert_rejected(encoded, "changes an authoritative callable contract");
}

#[test]
fn forged_mutated_capture_cannot_hide_a_borrowed_source() {
    let source = "type Reader = Callable[def() -> int64]\n\ndef main():\n    values: list[int64] = [1, 2]\n    read: def() -> int64 = lambda [values]: values.len()\n    print(read())\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let closure = main["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
        .find(|instruction| instruction["Assign"]["value"].get("Closure").is_some())
        .expect("the lambda lowers to a closure value");
    let capture = &mut closure["Assign"]["value"]["Closure"]["captures"][0];
    assert_eq!(capture["passing"], json!("Borrow"));
    // Claim the borrowed capture is environment-owned and mutated: the
    // closure declaration still takes it as a shared loan, so the forged
    // capture no longer matches its declaration.
    capture["passing"] = json!("Value");
    capture["mutated"] = json!(true);
    capture["source_place"] = Value::Null;
    assert_rejected(encoded, "closure");
}

#[test]
fn packed_values_run_with_backend_parity() {
    let mir = lower_source_to_mir(MUTABLE_SOURCE).expect("packing lowers");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, "1\n");
    emit_host_native_object(&mir).expect("the direct backend accepts the packed value");
}
