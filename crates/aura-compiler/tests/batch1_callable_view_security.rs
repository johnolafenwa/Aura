//! Forged-MIR regressions for stored argument-origin view contracts (C9,
//! Q22 A). A callable value's result position carries the declaration's
//! `view [mut] T from name` contract by origin ordinal. The shared validator
//! must refuse an operand that drops or strengthens that contract, a returned
//! loan that forges mutability over a shared contract, and a loan projection
//! outside the pointee type.

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

/// Every instruction of `function`, in block order; `get_mut` chains never
/// insert keys into an encoded instruction.
fn instructions_mut<'a>(function: &'a mut Value) -> impl Iterator<Item = &'a mut Value> + 'a {
    function["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
}

fn function_operand_mut<'a>(function: &'a mut Value, name: &str) -> &'a mut Value {
    instructions_mut(function)
        .find_map(|instruction| {
            instruction
                .get_mut("Assign")
                .and_then(|assign| assign.get_mut("value"))
                .and_then(|value| value.get_mut("Use"))
                .and_then(|operand| operand.get_mut("Function"))
                .filter(|function| function["name"] == name)
        })
        .unwrap_or_else(|| panic!("`{name}` should be stored as a function operand"))
}

fn returned_loan_mut<'a>(function: &'a mut Value, loan: &str) -> &'a mut Value {
    instructions_mut(function)
        .find_map(|instruction| {
            instruction
                .get_mut("BeginReturnedLoan")
                .filter(|begin| begin["loan"] == loan)
        })
        .unwrap_or_else(|| panic!("loan `{loan}` should begin a returned loan"))
}

const SHARED_SOURCE: &str = "class Pair:\n    left: str\n    count: int64\n\ndef pick_left(pair: Pair) -> view str from pair:\n    return view pair.left\n\ndef main():\n    pair = Pair(left=\"ada\", count=3)\n    chooser: def(pair: Pair) -> view str from pair = pick_left\n    view head = chooser(pair)\n    print(head)\n";

#[test]
fn forged_operand_cannot_drop_a_view_contract() {
    let mut encoded = encode(SHARED_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let operand = function_operand_mut(main, "pick_left");
    let return_type = operand
        .get_mut("signature")
        .and_then(|signature| signature.get_mut("Function"))
        .and_then(|function| function.get_mut("return_type"))
        .expect("the operand carries a structural signature");
    assert_eq!(return_type["ReturnedView"]["origin"], json!(0));
    // Claim the function returns an owned `str`: the declaration still
    // returns a view of `pair`, so the operand no longer matches it.
    *return_type = json!({"Named": ["str", []]});
    assert_rejected(
        encoded,
        "returned-view contract that does not match declaration",
    );
}

#[test]
fn forged_operand_cannot_strengthen_a_view_contract_to_mutable() {
    let mut encoded = encode(SHARED_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let operand = function_operand_mut(main, "pick_left");
    let view = operand
        .get_mut("signature")
        .and_then(|signature| signature.get_mut("Function"))
        .and_then(|function| function.get_mut("return_type"))
        .and_then(|return_type| return_type.get_mut("ReturnedView"))
        .expect("the operand carries a returned-view contract");
    assert_eq!(view["mutable"], json!(false));
    view["mutable"] = json!(true);
    assert_rejected(
        encoded,
        "returned-view contract that does not match declaration",
    );
}

#[test]
fn forged_returned_loan_cannot_take_mutable_capability_from_a_shared_contract() {
    let mut encoded = encode(SHARED_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let loan = returned_loan_mut(main, "head");
    assert_eq!(loan["mutable"], json!(false));
    loan["mutable"] = json!(true);
    assert_rejected(encoded, "forges mutable capability");
}

#[test]
fn forged_returned_loan_cannot_project_outside_the_pointee_type() {
    let mut encoded = encode(SHARED_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let loan = returned_loan_mut(main, "head");
    assert_eq!(loan["projections"], json!(["left"]));
    // `count` is an `int64` field of the origin: a `str` view of it would
    // read the wrong type, so the loan type check must refuse it.
    loan["projections"] = json!(["left", "count"]);
    assert_rejected(encoded, "expected");
}

#[test]
fn stored_view_contracts_run_with_backend_parity() {
    let mir = lower_source_to_mir(SHARED_SOURCE).expect("stored view contracts lower");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, "ada\n");
    emit_host_native_object(&mir).expect("the direct backend accepts the stored view contract");
}

const BOUND_SOURCE: &str = "class Pair:\n    left: str\n    right: str\n\nclass Chooser:\n    prefer_left: bool\n\n    def pick(self, pair: Pair) -> view str from pair:\n        if self.prefer_left:\n            return view pair.left\n        return view pair.right\n\ndef main():\n    pair = Pair(left=\"ada\", right=\"linus\")\n    chooser = Chooser(prefer_left=false)\n    picker = chooser.pick\n    view chosen = picker(pair)\n    print(chosen)\n";

fn closure_signature_view_mut(function: &mut Value) -> &mut Value {
    instructions_mut(function)
        .find_map(|instruction| {
            instruction
                .get_mut("Assign")
                .and_then(|assign| assign.get_mut("value"))
                .and_then(|value| value.get_mut("Closure"))
                .and_then(|closure| closure.get_mut("signature"))
                .and_then(|signature| signature.get_mut("Closure"))
                .and_then(|closure| closure.get_mut("return_type"))
                .and_then(|return_type| return_type.get_mut("ReturnedView"))
        })
        .expect("the bound method's closure carries a returned-view contract")
}

#[test]
fn forged_bound_method_closure_cannot_change_its_view_contract() {
    for (field, forged) in [("mutable", json!(true)), ("origin", json!(1))] {
        let mut encoded = encode(BOUND_SOURCE);
        let main = function_mut(&mut encoded, "main");
        let view = closure_signature_view_mut(main);
        assert_ne!(view[field], forged);
        // The generated function returns a shared view of its first exposed
        // parameter; a closure claiming otherwise no longer matches it.
        view[field] = forged;
        assert_rejected(encoded, "changes its declared callable contract");
    }
}

#[test]
fn bound_method_view_contracts_run_with_backend_parity() {
    let mir = lower_source_to_mir(BOUND_SOURCE).expect("bound view contracts lower");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, "linus\n");
    emit_host_native_object(&mir).expect("the direct backend accepts the bound view contract");
}
