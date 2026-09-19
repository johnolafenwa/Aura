//! Forged-MIR regressions for the explicit callable adapter operation
//! (`CallableAdapt`, C5 / Q17 A). The adapter is the only MIR operation that
//! may restrict an exposed callable contract, so the shared validator must
//! refuse every adapter whose source, destination, or declared target
//! disagrees: a source without an authoritative identity or that is not a
//! callable, a thin adapter over an environment or an inadmissible contract,
//! a contextual lambda or packing adapter that changes the contract or call
//! kind, a packed loan capture, a task packing without a Transfer
//! environment, a non-callable destination, and a destination that differs
//! from the typed local. Both public boundaries must report the same reason.

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

/// The `Assign` whose value is the first `CallableAdapt` in `function`.
fn adapter_assign_mut(function: &mut Value) -> &mut Value {
    for block in function["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction.pointer("/Assign/value/CallableAdapt").is_some() {
                return &mut instruction["Assign"];
            }
        }
    }
    panic!("expected a callable adapter instruction");
}

/// The `Assign` whose value is the first closure literal in `function`.
fn closure_assign_mut(function: &mut Value) -> &mut Value {
    for block in function["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction.pointer("/Assign/value/Closure").is_some() {
                return &mut instruction["Assign"];
            }
        }
    }
    panic!("expected a closure literal instruction");
}

fn local_type_mut<'a>(function: &'a mut Value, local: &str) -> &'a mut Value {
    let entry = function["local_types"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["name"] == local)
        .unwrap_or_else(|| panic!("local `{local}` should have a type"));
    &mut entry["ty"]
}

fn int_type() -> Value {
    json!({ "Named": ["int64", []] })
}

fn str_type() -> Value {
    json!({ "Named": ["str", []] })
}

fn task_group_type() -> Value {
    json!({ "Named": ["TaskGroup", []] })
}

fn assert_accepted_on_both_boundaries(source: &str, stdout: &str) {
    let mir = lower_source_to_mir(source).expect("source should lower");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, stdout);
    emit_host_native_object(&mir).expect("the direct backend accepts the program");
}

fn assert_rejected_on_both_boundaries(encoded: Value, expected: &str) {
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let interpreted = run_mir(&mir).expect_err("interpreter must reject the forged module");
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert_eq!(
        interpreted.message.strip_prefix("invalid MIR loan flow: "),
        Some(native.as_str()),
        "both boundaries must report the same shared validator reason"
    );
    assert!(
        native.contains(expected),
        "shared rejection `{native}` should mention `{expected}`"
    );
}

/// A thin alias adapter over a named function (`Unary(increment)`).
const THIN: &str = "type Unary = def(int64) -> int64\n\ndef increment(value: int64 = 1) -> int64:\n    return value + 1\n\ndef main():\n    step: Unary = Unary(increment)\n    print(step(4))\n";

/// An owned packing adapter over a capturing closure (`Tool(lambda ...)`).
const PACKED: &str = "type Tool = Callable[def(value: int64) -> int64]\n\ndef make(offset: int64) -> Tool:\n    base = offset\n    return Tool(lambda value: value + base)\n\ndef main():\n    tool = make(2)\n    print(tool(value=7))\n";

/// A task packing adapter over a capturing closure (`Job(lambda ...)`).
const JOB: &str = "type Job = TaskCallable[def() -> int64]\n\ndef main():\n    base = 1\n    target = Job(lambda: base)\n    with TaskGroup() as group:\n        task = group.start(target)\n        print(task.result_or(-1, timeout=1s))\n";

#[test]
fn explicit_adapters_run_on_both_boundaries() {
    assert_accepted_on_both_boundaries(THIN, "5\n");
    assert_accepted_on_both_boundaries(PACKED, "9\n");
    assert_accepted_on_both_boundaries(JOB, "1\n");
}

#[test]
fn adapter_from_a_place_without_an_identity_is_rejected_on_both_boundaries() {
    let mut encoded = encode(THIN);
    let main = function_mut(&mut encoded, "main");
    // Any plain integer local: it has a declared type but no recorded
    // callable identity, and declared metadata is never an identity.
    let plain = main["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["ty"] == int_type())
        .expect("main has an integer local")["name"]
        .clone();
    adapter_assign_mut(main)["value"]["CallableAdapt"]["value"] = json!({ "Place": plain });
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR callable adapter in `main` has no authoritative source contract",
    );
}

#[test]
fn adapter_from_a_non_callable_operand_is_rejected_on_both_boundaries() {
    let mut encoded = encode(THIN);
    adapter_assign_mut(function_mut(&mut encoded, "main"))["value"]["CallableAdapt"]["value"] =
        json!({ "Int": 1 });
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR callable adapter in `main` has a non-callable source",
    );
}

#[test]
fn thin_adapter_over_a_capturing_closure_is_rejected_on_both_boundaries() {
    let mut encoded = encode(PACKED);
    let make = function_mut(&mut encoded, "make");
    let assign = adapter_assign_mut(make);
    let target = assign["target"].as_str().unwrap().to_owned();
    let callable = assign["value"]["CallableAdapt"]["destination"]["Callable"].clone();
    let thin = json!({ "Function": {
        "params": callable["params"],
        "return_type": callable["return_type"],
    }});
    assign["value"]["CallableAdapt"]["destination"] = thin.clone();
    *local_type_mut(make, &target) = thin;
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR thin callable adapter in `make` captures an environment",
    );
}

#[test]
fn thin_adapter_that_changes_a_parameter_type_is_rejected_on_both_boundaries() {
    let mut encoded = encode(THIN);
    let main = function_mut(&mut encoded, "main");
    let assign = adapter_assign_mut(main);
    let target = assign["target"].as_str().unwrap().to_owned();
    assign["value"]["CallableAdapt"]["destination"]["Function"]["params"][0]["ty"] = str_type();
    local_type_mut(main, &target)["Function"]["params"][0]["ty"] = str_type();
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR thin callable adapter in `main` changes an inadmissible contract",
    );
}

#[test]
fn contextual_lambda_adapter_that_changes_a_parameter_type_is_rejected_on_both_boundaries() {
    let mut encoded = encode(PACKED);
    let make = function_mut(&mut encoded, "make");
    let mut destination = closure_assign_mut(make)["value"]["Closure"]["signature"].clone();
    destination["Closure"]["params"][0]["ty"] = str_type();
    let assign = adapter_assign_mut(make);
    let target = assign["target"].as_str().unwrap().to_owned();
    assign["value"]["CallableAdapt"]["destination"] = destination.clone();
    *local_type_mut(make, &target) = destination;
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR contextual lambda adapter in `make` changes an inadmissible contract",
    );
}

#[test]
fn packing_that_changes_a_parameter_type_is_rejected_on_both_boundaries() {
    let mut encoded = encode(PACKED);
    let make = function_mut(&mut encoded, "make");
    let assign = adapter_assign_mut(make);
    let target = assign["target"].as_str().unwrap().to_owned();
    assign["value"]["CallableAdapt"]["destination"]["Callable"]["params"][0]["ty"] = str_type();
    local_type_mut(make, &target)["Callable"]["params"][0]["ty"] = str_type();
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR callable packing in `make` changes an inadmissible contract or call kind",
    );
}

#[test]
fn packing_a_loan_capture_is_rejected_on_both_boundaries() {
    let mut encoded = encode(PACKED);
    let make = function_mut(&mut encoded, "make");
    let closure = closure_assign_mut(make);
    let target = closure["target"].as_str().unwrap().to_owned();
    closure["value"]["Closure"]["signature"]["Closure"]["captures"][0]["mode"] =
        json!("SharedView");
    local_type_mut(make, &target)["Closure"]["captures"][0]["mode"] = json!("SharedView");
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR callable packing in `make` stores a loan capture",
    );
}

#[test]
fn task_packing_of_a_host_resource_capture_is_rejected_on_both_boundaries() {
    let mut encoded = encode(JOB);
    // Type the captured `base` as a TaskGroup consistently: the closure
    // literal, its declared local, the capturing lambda's declaration, and
    // the captured local itself, so only the packing's Transfer proof can
    // refuse the adapter.
    let main = function_mut(&mut encoded, "main");
    let closure = closure_assign_mut(main);
    let target = closure["target"].as_str().unwrap().to_owned();
    let lambda = closure["value"]["Closure"]["function"]
        .as_str()
        .unwrap()
        .to_owned();
    closure["value"]["Closure"]["signature"]["Closure"]["captures"][0]["ty"] = task_group_type();
    closure["value"]["Closure"]["captures"][0]["ty"] = task_group_type();
    local_type_mut(main, &target)["Closure"]["captures"][0]["ty"] = task_group_type();
    *local_type_mut(main, "base") = task_group_type();
    let lambda = function_mut(&mut encoded, &lambda);
    for param in lambda["params"].as_array_mut().unwrap() {
        if param["name"] == "base" {
            param["ty"] = task_group_type();
        }
    }
    *local_type_mut(lambda, "base") = task_group_type();
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR task-callable packing in `main` lacks a Transfer environment",
    );
}

#[test]
fn adapter_to_a_non_callable_destination_is_rejected_on_both_boundaries() {
    let mut encoded = encode(THIN);
    let main = function_mut(&mut encoded, "main");
    let assign = adapter_assign_mut(main);
    let target = assign["target"].as_str().unwrap().to_owned();
    assign["value"]["CallableAdapt"]["destination"] = int_type();
    *local_type_mut(main, &target) = int_type();
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR callable adapter in `main` has a non-callable destination",
    );
}

#[test]
fn adapter_whose_destination_differs_from_its_local_type_is_rejected_on_both_boundaries() {
    let mut encoded = encode(THIN);
    adapter_assign_mut(function_mut(&mut encoded, "main"))["value"]["CallableAdapt"]
        ["destination"]["Function"]["params"][0]["name"] = json!("x");
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR callable adapter in `main` does not match its destination type",
    );
}
