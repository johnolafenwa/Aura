//! Forged-MIR regressions for bound methods (C6, Q18 A). `receiver.method`
//! is a compiler-synthesized closure whose single capture is the receiver.
//! The shared validator must refuse a forged closure declaration that hides
//! the receiver's mutable environment ownership, and a forged associated
//! method value that changes the method's contract.

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

fn assign_rvalue_mut<'a>(instruction: &'a mut Value, rvalue: &str) -> Option<&'a mut Value> {
    // `get_mut` chains never insert keys; indexing a map with a missing key
    // would add a null entry and corrupt the encoded instruction.
    instruction
        .get_mut("Assign")
        .and_then(|assign| assign.get_mut("value"))
        .and_then(|value| value.get_mut(rvalue))
}

fn closure_rvalue_mut(function: &mut Value) -> &mut Value {
    function["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
        .find_map(|instruction| assign_rvalue_mut(instruction, "Closure"))
        .expect("the bound method lowers to a closure value")
}

const MUTABLE_SOURCE: &str = "class Counter:\n    total: int64\n\n    def bump(mut self, by: int64 = 1) -> int64:\n        self.total = self.total + by\n        return self.total\n\ndef main():\n    counter = Counter(total=10)\n    mut bump = counter.bump\n    print(bump())\n    print(bump(by=5))\n";

#[test]
fn forged_bound_method_cannot_hide_its_mutable_receiver() {
    let mut encoded = encode(MUTABLE_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let closure = closure_rvalue_mut(main);
    assert_eq!(closure["mutable"], json!(true));
    assert_eq!(closure["captures"][0]["name"], json!("__receiver"));
    assert_eq!(closure["captures"][0]["mutated"], json!(true));
    // Claim the receiver is a plain owned capture of a Repeatable closure:
    // the synthesized function still takes `__receiver` mutably, so the
    // forged capture no longer matches its declaration.
    closure["mutable"] = json!(false);
    closure["captures"][0]["mutated"] = json!(false);
    assert_rejected(encoded, "closure");
}

#[test]
fn forged_bound_method_cannot_strip_the_mutable_call_kind() {
    let mut encoded = encode(MUTABLE_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let closure = closure_rvalue_mut(main);
    // Keep the mutated capture but declare the closure value Repeatable: the
    // signature's call kind would let shared storage mutate the receiver.
    closure["mutable"] = json!(false);
    assert_rejected(encoded, "closure");
}

const CONSUMING_SOURCE: &str = "class Ticket:\n    code: str\n\n    def redeem(own self) -> str:\n        return self.code\n\ndef main():\n    ticket = Ticket(code=\"vip\")\n    redeem = ticket.redeem\n    print(redeem())\n";

#[test]
fn forged_bound_method_cannot_make_a_consuming_receiver_repeatable() {
    let mut encoded = encode(CONSUMING_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let closure = closure_rvalue_mut(main);
    assert_eq!(closure["consuming"], json!(true));
    closure["consuming"] = json!(false);
    assert_rejected(encoded, "closure");
}

#[test]
fn bound_methods_run_with_backend_parity() {
    for (source, expected) in [(MUTABLE_SOURCE, "11\n16\n"), (CONSUMING_SOURCE, "vip\n")] {
        let mir = lower_source_to_mir(source).expect("bound methods lower");
        assert_eq!(run_mir(&mir).expect("the program runs").stdout, expected);
        emit_host_native_object(&mir).expect("the direct backend accepts the bound method");
    }
}

const ASSOCIATED_SOURCE: &str = "class Math:\n    def double(value: int64) -> int64:\n        return value * 2\n\ndef apply(step: def(value: int64) -> int64, value: int64) -> int64:\n    return step(value)\n\ndef main():\n    print(apply(Math.double, 4))\n";

#[test]
fn forged_associated_method_value_cannot_change_its_contract() {
    let mut encoded = encode(ASSOCIATED_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let function_value = main["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
        .flat_map(|instruction| {
            assign_rvalue_mut(instruction, "Call")
                .and_then(|call| call.get_mut("args"))
                .and_then(|args| args.as_array_mut())
                .map(|args| args.iter_mut())
                .into_iter()
                .flatten()
        })
        .find_map(|arg| {
            arg.get_mut("value")
                .and_then(|value| value.get_mut("Function"))
        })
        .expect("the associated method lowers to a function value");
    assert_eq!(function_value["name"], json!("Math.double"));
    let return_type = function_value
        .get_mut("signature")
        .and_then(|signature| signature.get_mut("Function"))
        .and_then(|function| function.get_mut("return_type"))
        .expect("the function value carries a structural signature");
    assert_eq!(*return_type, json!({"Named": ["int64", []]}));
    *return_type = json!({"Named": ["str", []]});
    assert_rejected(encoded, "Math.double");
}

#[test]
fn associated_method_values_run_with_backend_parity() {
    let mir = lower_source_to_mir(ASSOCIATED_SOURCE).expect("associated method values lower");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, "8\n");
    emit_host_native_object(&mir).expect("the direct backend accepts the function value");
}
