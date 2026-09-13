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

fn tuple_of_task_group_type() -> Value {
    json!({ "Tuple": [task_group_type()] })
}

#[test]
fn named_target_result_typed_as_a_host_resource_is_rejected_on_both_boundaries() {
    let mut encoded = encode(NAMED_TARGET);
    function_mut(&mut encoded, "worker")["return_type"] = task_group_type();
    let main = function_mut(&mut encoded, "main");
    start_task_mut(main)["function"]["Function"]["signature"]["Function"]["return_type"] =
        task_group_type();
    assert_rejected(
        encoded,
        "returns a result whose type is not Transfer: `TaskGroup` is a host resource",
    );
}

#[test]
fn named_target_parameter_typed_as_a_module_capability_is_rejected_on_both_boundaries() {
    let mut encoded = encode(NAMED_TARGET);
    let module = json!({ "Module": "api" });
    function_mut(&mut encoded, "worker")["params"][0]["ty"] = module.clone();
    let main = function_mut(&mut encoded, "main");
    let argument = start_task_mut(main)["args"][0]["value"]["Place"]
        .as_str()
        .expect("the task argument should be a place")
        .to_owned();
    start_task_mut(main)["function"]["Function"]["signature"]["Function"]["params"][0]["ty"] =
        module.clone();
    set_local_type(main, &argument, module.clone());
    set_local_type(main, "value", module);
    assert_rejected(
        encoded,
        "passes parameter 1 whose type is not Transfer: `module api` is a module capability",
    );
}

#[test]
fn named_target_parameter_carrying_a_host_resource_in_a_tuple_is_rejected_on_both_boundaries() {
    let mut encoded = encode(NAMED_TARGET);
    function_mut(&mut encoded, "worker")["params"][0]["ty"] = tuple_of_task_group_type();
    let main = function_mut(&mut encoded, "main");
    let argument = start_task_mut(main)["args"][0]["value"]["Place"]
        .as_str()
        .expect("the task argument should be a place")
        .to_owned();
    start_task_mut(main)["function"]["Function"]["signature"]["Function"]["params"][0]["ty"] =
        tuple_of_task_group_type();
    set_local_type(main, &argument, tuple_of_task_group_type());
    set_local_type(main, "value", tuple_of_task_group_type());
    assert_rejected(
        encoded,
        "passes parameter 1 whose type is not Transfer: `TaskGroup` is a host resource",
    );
}

const ENUM_ARGUMENT: &str = "enum Slot:\n    Filled(int64)\n    Empty\n\ndef worker(slot: Slot) -> int64:\n    match slot:\n        case Slot.Filled(value):\n            return value\n        case Slot.Empty:\n            return 0\n\ndef main():\n    slot = Slot.Filled(1)\n    with TaskGroup() as group:\n        task = group.start(worker, slot)\n        print(task.result_or(-1, timeout=1s))\n";

#[test]
fn named_target_parameter_whose_enum_payload_is_a_host_resource_is_rejected_on_both_boundaries() {
    let mut encoded = encode(ENUM_ARGUMENT);
    // The enum itself stays the parameter type; its `Filled` payload is
    // forged into a host resource, so only the recursive Transfer proof over
    // the module's enum metadata can refuse the start.
    let slot = encoded["enums"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|enum_decl| enum_decl["name"] == "Slot")
        .expect("enum `Slot` should be lowered");
    let filled = slot["variants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|variant| variant["name"] == "Filled")
        .expect("variant `Filled` should be lowered");
    filled["payloads"][0] = task_group_type();
    assert_rejected(
        encoded,
        "payload of `Slot.Filled`: `TaskGroup` is a host resource",
    );
}

const CLASS_ARGUMENT: &str = "class Holder:\n    count: int64\n\ndef worker(holder: Holder) -> int64:\n    return holder.count\n\ndef main():\n    holder = Holder(count=1)\n    with TaskGroup() as group:\n        task = group.start(worker, holder)\n        print(task.result_or(-1, timeout=1s))\n";

#[test]
fn named_target_parameter_whose_class_field_is_a_host_resource_is_rejected_on_both_boundaries() {
    let mut encoded = encode(CLASS_ARGUMENT);
    let holder = encoded["classes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|class| class["name"] == "Holder")
        .expect("class `Holder` should be lowered");
    holder["fields"][0]["ty"] = task_group_type();
    assert_rejected(
        encoded,
        "field `count` of `Holder`: `TaskGroup` is a host resource",
    );
}

#[test]
fn closure_target_capturing_a_live_view_is_rejected_on_both_boundaries() {
    let mut encoded = encode(CLOSURE_TARGET);
    let main = function_mut(&mut encoded, "main");
    let closure = find_instruction(main, |instruction| {
        instruction.pointer("/Assign/value/Closure").is_some()
    });
    let target = closure["Assign"]["target"].as_str().unwrap().to_owned();
    closure["Assign"]["value"]["Closure"]["signature"]["Closure"]["captures"][0]["mode"] =
        json!("SharedView");
    set_local_type(
        main,
        &target,
        json!({ "Closure": {
            "params": [],
            "return_type": { "Named": ["int64", []] },
            "captures": [{ "name": "base", "ty": { "Named": ["int64", []] }, "mode": "SharedView", "span": { "line": 4, "column": 36 }, "mutated": false }],
            "call_kind": "Repeatable"
        }}),
    );
    assert_rejected(
        encoded,
        "starts a target whose environment is not Transfer: capture `base` is a live loan",
    );
}

#[test]
fn stored_target_parameter_typed_as_a_returned_view_is_rejected_on_both_boundaries() {
    let mut encoded = encode(STORED_TARGET);
    let view = json!({
        "ReturnedView": { "mutable": false, "pointee": { "Named": ["int64", []] }, "origin": 0 }
    });
    function_mut(&mut encoded, "worker")["params"][0]["ty"] = view.clone();
    let default_function = function_mut(&mut encoded, "worker")["params"][0]["default_function"]
        .as_str()
        .map(str::to_owned);
    if let Some(default_function) = default_function {
        function_mut(&mut encoded, &default_function)["return_type"] = view.clone();
    }
    let main = function_mut(&mut encoded, "main");
    for block in main["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if let Some(signature) = instruction.pointer_mut("/Assign/value/Use/Function/signature")
            {
                signature["Function"]["params"][0]["ty"] = view.clone();
            }
        }
    }
    for entry in main["local_types"].as_array_mut().unwrap() {
        if let Some(params) = entry.pointer_mut("/ty/Callable/params") {
            params[0]["ty"] = view.clone();
        }
    }
    assert_rejected(
        encoded,
        "passes parameter 1 whose type is not Transfer: a returned view borrows the parent's data",
    );
}

#[test]
fn named_target_result_typed_as_an_erased_callable_is_rejected_on_both_boundaries() {
    let mut encoded = encode(NAMED_TARGET);
    let erased = json!({ "Callable": {
        "task": false,
        "call_kind": "Repeatable",
        "params": [],
        "return_type": { "Named": ["int64", []] }
    }});
    function_mut(&mut encoded, "worker")["return_type"] = erased.clone();
    let main = function_mut(&mut encoded, "main");
    start_task_mut(main)["function"]["Function"]["signature"]["Function"]["return_type"] = erased;
    assert_rejected(
        encoded,
        "returns a result whose type is not Transfer: an erased `Callable` hides its environment",
    );
}
