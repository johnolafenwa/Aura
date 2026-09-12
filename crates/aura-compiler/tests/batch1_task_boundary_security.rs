//! Forged-MIR regressions for the task boundary (C8): the shared validator
//! must refuse a task start whose target contract carries a non-Transfer
//! parameter (supplied or defaulted), a returned view, or a non-Transfer
//! captured environment, and both public boundaries (the interpreter and the
//! direct backend) must reject with the same validator reason.

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

fn find_instruction(function: &mut Value, matches: impl Fn(&Value) -> bool) -> &mut Value {
    for block in function["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if matches(instruction) {
                return instruction;
            }
        }
    }
    panic!("expected instruction not found");
}

fn start_task_mut(function: &mut Value) -> &mut Value {
    let instruction = find_instruction(function, |instruction| {
        instruction.pointer("/Assign/value/StartTask").is_some()
    });
    &mut instruction["Assign"]["value"]["StartTask"]
}

fn set_local_type(function: &mut Value, local: &str, ty: Value) {
    let entry = function["local_types"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["name"] == local)
        .unwrap_or_else(|| panic!("local `{local}` should have a type"));
    entry["ty"] = ty;
}

fn task_group_type() -> Value {
    json!({ "Named": ["TaskGroup", []] })
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

const NAMED_TARGET: &str = "def worker(count: int64) -> int64:\n    return count\n\ndef main():\n    value = 1\n    with TaskGroup() as group:\n        task = group.start(worker, value)\n        print(task.result_or(-1, timeout=1s))\n";

#[test]
fn named_target_parameter_typed_as_a_host_resource_is_rejected_on_both_boundaries() {
    let mut encoded = encode(NAMED_TARGET);
    // Make the declaration, the operand contract, and the argument agree on
    // a `TaskGroup` parameter so only the Transfer rule can refuse the start.
    function_mut(&mut encoded, "worker")["params"][0]["ty"] = task_group_type();
    let main = function_mut(&mut encoded, "main");
    let argument = start_task_mut(main)["args"][0]["value"]["Place"]
        .as_str()
        .expect("the task argument should be a place")
        .to_owned();
    start_task_mut(main)["function"]["Function"]["signature"]["Function"]["params"][0]["ty"] =
        task_group_type();
    set_local_type(main, &argument, task_group_type());
    set_local_type(main, "value", task_group_type());
    assert_rejected(
        encoded,
        "passes parameter 1 whose type is not Transfer: `TaskGroup` is a host resource",
    );
}

const STORED_TARGET: &str = "type Job = TaskCallable[def(count: int64 = ...) -> int64]\n\ndef worker(count: int64 = 2) -> int64:\n    return count\n\ndef main():\n    target = Job(worker)\n    with TaskGroup() as group:\n        task = group.start(target)\n        print(task.result_or(-1, timeout=1s))\n";

#[test]
fn stored_target_defaulted_parameter_typed_as_a_host_resource_is_rejected_on_both_boundaries() {
    let mut encoded = encode(STORED_TARGET);
    // The omitted default never appears in the argument list; the contract
    // slot itself must be proved Transfer.
    function_mut(&mut encoded, "worker")["params"][0]["ty"] = task_group_type();
    let default_function = function_mut(&mut encoded, "worker")["params"][0]["default_function"]
        .as_str()
        .map(str::to_owned);
    if let Some(default_function) = default_function {
        function_mut(&mut encoded, &default_function)["return_type"] = task_group_type();
    }
    let main = function_mut(&mut encoded, "main");
    for block in main["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if let Some(signature) = instruction.pointer_mut("/Assign/value/Use/Function/signature")
            {
                signature["Function"]["params"][0]["ty"] = task_group_type();
            }
        }
    }
    for entry in main["local_types"].as_array_mut().unwrap() {
        if let Some(params) = entry.pointer_mut("/ty/Callable/params") {
            params[0]["ty"] = task_group_type();
        }
    }
    assert_rejected(
        encoded,
        "passes parameter 1 whose type is not Transfer: `TaskGroup` is a host resource",
    );
}

const VIEW_DECLARATION: &str = "class Pair:\n    left: str\n\ndef pick_left(pair: Pair) -> view str from pair:\n    return view pair.left\n\ndef other(pair: Pair) -> str:\n    return pair.left.clone()\n\ndef main():\n    pair = Pair(left=\"ada\")\n    second = Pair(left=\"linus\")\n    with TaskGroup() as group:\n        task = group.start(other, pair)\n        print(task.result_or(\"fallback\", timeout=1s))\n    view head = pick_left(second)\n    print(head)\n";

#[test]
fn named_target_with_an_authentic_view_contract_is_rejected_on_both_boundaries() {
    let mut encoded = encode(VIEW_DECLARATION);
    let main = function_mut(&mut encoded, "main");
    let start = start_task_mut(main);
    // Redirect the start at the view-returning declaration with its authentic
    // signature, exactly as a checked program could never spell it.
    start["function"]["Function"]["name"] = json!("pick_left");
    start["function"]["Function"]["signature"]["Function"]["return_type"] = json!({
        "ReturnedView": { "mutable": false, "pointee": { "Named": ["str", []] }, "origin": 0 }
    });
    assert_rejected(
        encoded,
        "starts a target that returns a view of its arguments",
    );
}

const CLOSURE_TARGET: &str = "def main():\n    base = 41\n    with TaskGroup() as group:\n        task = group.start(lambda: base + 1)\n        print(task.result_or(-1, timeout=1s))\n";

#[test]
fn closure_target_capturing_a_host_resource_is_rejected_on_both_boundaries() {
    let mut encoded = encode(CLOSURE_TARGET);
    let main = function_mut(&mut encoded, "main");
    let closure = find_instruction(main, |instruction| {
        instruction.pointer("/Assign/value/Closure").is_some()
    });
    let target = closure["Assign"]["target"].as_str().unwrap().to_owned();
    let lambda = closure["Assign"]["value"]["Closure"]["function"]
        .as_str()
        .unwrap()
        .to_owned();
    closure["Assign"]["value"]["Closure"]["signature"]["Closure"]["captures"][0]["ty"] =
        task_group_type();
    closure["Assign"]["value"]["Closure"]["captures"][0]["ty"] = task_group_type();
    // The lambda's own declaration receives the capture as a parameter; keep
    // it consistent so only the boundary rule can refuse the start.
    for param in function_mut(&mut encoded, &lambda)["params"]
        .as_array_mut()
        .unwrap()
    {
        if param["name"] == "base" {
            param["ty"] = task_group_type();
        }
    }
    let main = function_mut(&mut encoded, "main");
    set_local_type(main, "base", task_group_type());
    set_local_type(
        main,
        &target,
        json!({ "Closure": {
            "params": [],
            "return_type": { "Named": ["int64", []] },
            "captures": [{ "name": "base", "ty": task_group_type(), "mode": "Copy", "span": { "line": 4, "column": 36 }, "mutated": false }],
            "call_kind": "Repeatable"
        }}),
    );
    assert_rejected(
        encoded,
        "starts a target whose environment is not Transfer: capture `base`: `TaskGroup` is a host resource",
    );
}
