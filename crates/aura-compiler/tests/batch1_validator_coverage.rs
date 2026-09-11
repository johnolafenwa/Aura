//! Forged-MIR regressions for the shared loan-flow validator's structural
//! rejections. Each case lowers a valid program, corrupts one MIR fact in the
//! serialized module, and asserts that both public boundaries (the
//! interpreter and the direct backend) reject it with the validator's message.

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

fn blocks_mut(function: &mut Value) -> &mut Vec<Value> {
    function["blocks"].as_array_mut().unwrap()
}

fn instructions_mut(block: &mut Value) -> &mut Vec<Value> {
    block["instructions"].as_array_mut().unwrap()
}

/// Every instruction of every block of a function, flattened for searching.
fn find_instruction(function: &mut Value, matches: impl Fn(&Value) -> bool) -> &mut Value {
    for block in blocks_mut(function) {
        for instruction in instructions_mut(block) {
            if matches(instruction) {
                return instruction;
            }
        }
    }
    panic!("expected instruction not found");
}

fn find_block<'a>(function: &'a mut Value, matches: impl Fn(&Value) -> bool) -> &'a mut Value {
    blocks_mut(function)
        .iter_mut()
        .find(|block| matches(block))
        .expect("expected block not found")
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

fn rvalue_kind(instruction: &Value) -> Option<&str> {
    instruction
        .get("Assign")
        .and_then(|assign| assign.get("value"))
        .and_then(|value| value.as_object())
        .and_then(|object| object.keys().next())
        .map(String::as_str)
}

const BRANCHING: &str =
    "def main():\n    flag = true\n    if flag:\n        print(1)\n    else:\n        print(2)\n";

#[test]
fn duplicate_block_labels_are_rejected() {
    let mut encoded = encode(BRANCHING);
    let main = function_mut(&mut encoded, "main");
    let first = blocks_mut(main)[0]["label"].clone();
    blocks_mut(main)[1]["label"] = first;
    assert_rejected(encoded, "has duplicate block label");
}

#[test]
fn missing_entry_block_is_rejected() {
    let mut encoded = encode(BRANCHING);
    function_mut(&mut encoded, "main")["entry"] = json!("nowhere");
    assert_rejected(encoded, "has missing entry block");
}

#[test]
fn join_with_inconsistent_active_loans_is_rejected() {
    let source = "def main():\n    values = [1]\n    flag = true\n    if flag:\n        view shared = values\n        print(shared.len())\n    else:\n        print(2)\n    print(values.len())\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    // Drop every EndLoan in the then-branch so the join sees one path with an
    // active loan and one without.
    for block in blocks_mut(main) {
        let label = block["label"].as_str().unwrap().to_owned();
        if label.contains("then") {
            instructions_mut(block).retain(|instruction| instruction.get("EndLoan").is_none());
        }
    }
    assert_rejected(encoded, "inconsistent active loans");
}

const ENUM_MATCH: &str = "enum Shape:\n    Circle(int64)\n    Square(int64)\ndef main():\n    shape = Shape.Circle(3)\n    match shape:\n        case Shape.Circle(radius):\n            print(radius)\n        case Shape.Square(side):\n            print(side)\n";

fn match_arms_mut(function: &mut Value) -> &mut Vec<Value> {
    let block = find_block(function, |block| block["terminator"].get("Match").is_some());
    block["terminator"]["Match"]["arms"].as_array_mut().unwrap()
}

#[test]
fn match_arm_for_the_wrong_enum_is_rejected() {
    let mut encoded = encode(ENUM_MATCH);
    match_arms_mut(function_mut(&mut encoded, "main"))[0]["enum_name"] = json!("Other");
    assert_rejected(encoded, "has an arm for the wrong enum");
}

#[test]
fn match_enum_arm_without_a_variant_is_rejected() {
    let mut encoded = encode(ENUM_MATCH);
    match_arms_mut(function_mut(&mut encoded, "main"))[0]["variant_name"] = Value::Null;
    assert_rejected(encoded, "has an enum arm without a variant");
}

#[test]
fn match_arm_naming_an_unknown_variant_is_rejected() {
    let mut encoded = encode(ENUM_MATCH);
    match_arms_mut(function_mut(&mut encoded, "main"))[0]["variant_name"] = json!("Triangle");
    assert_rejected(encoded, "names unknown variant `Triangle`");
}

const GENERIC_ENUM_MATCH: &str = "enum Box[T]:\n    Full(T)\n    Empty\ndef main():\n    boxed = Box.Full(3)\n    match boxed:\n        case Box.Full(value):\n            print(value)\n        case Box.Empty:\n            print(0)\n";

fn retype_local(function: &mut Value, local: &str, ty: Value) {
    for entry in function["local_types"].as_array_mut().unwrap() {
        if entry["name"] == local {
            entry["ty"] = ty.clone();
        }
    }
}

#[test]
fn match_on_an_enum_with_the_wrong_type_arity_is_rejected() {
    let mut encoded = encode(GENERIC_ENUM_MATCH);
    let main = function_mut(&mut encoded, "main");
    retype_local(main, "boxed", json!({ "Named": ["Box", []] }));
    assert_rejected(encoded, "uses enum `Box` with incorrect type arity");
}

#[test]
fn variant_payload_with_the_wrong_enum_type_arity_is_rejected() {
    let mut encoded = encode(GENERIC_ENUM_MATCH);
    let main = function_mut(&mut encoded, "main");
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t90", "ty": { "Named": ["Box", []] } }));
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("VariantPayload")
    });
    assign["Assign"]["value"]["VariantPayload"]["scrutinee"] = json!({ "Place": "%t90" });
    assert_rejected(encoded, "uses enum `Box` with incorrect type arity");
}

#[test]
fn variant_payload_from_a_non_place_scrutinee_is_rejected() {
    let mut encoded = encode(ENUM_MATCH);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("VariantPayload")
    });
    assign["Assign"]["value"]["VariantPayload"]["scrutinee"] = json!({ "Int": 1 });
    assert_rejected(encoded, "requires a place scrutinee");
}

#[test]
fn variant_payload_from_a_class_typed_scrutinee_is_rejected() {
    let source = "class Holder:\n    value: int64\nenum Shape:\n    Circle(int64)\n    Square(int64)\ndef main():\n    holder = Holder(value=1)\n    shape = Shape.Circle(3)\n    match shape:\n        case Shape.Circle(radius):\n            print(radius)\n        case Shape.Square(side):\n            print(side)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("VariantPayload")
    });
    assign["Assign"]["value"]["VariantPayload"]["scrutinee"] = json!({ "Place": "holder" });
    assert_rejected(encoded, "requires an enum scrutinee");
}

#[test]
fn duplicate_enum_declarations_are_rejected() {
    let mut encoded = encode(ENUM_MATCH);
    let duplicate = encoded["enums"][0].clone();
    encoded["enums"].as_array_mut().unwrap().push(duplicate);
    assert_rejected(encoded, "has duplicate enum `Shape`");
}

#[test]
fn duplicate_enum_type_parameters_are_rejected() {
    let mut encoded = encode(GENERIC_ENUM_MATCH);
    encoded["enums"][0]["type_params"] = json!(["T", "T"]);
    assert_rejected(encoded, "has duplicate type parameter `T`");
}

#[test]
fn enum_place_projection_without_a_payload_projection_is_rejected() {
    let mut encoded = encode(ENUM_MATCH);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("VariantPayload")
    });
    assign["Assign"]["value"]["VariantPayload"]["scrutinee"] = json!({ "Place": "shape.radius" });
    assert_rejected(encoded, "without a canonical payload projection");
}

const UNION_MATCH: &str = "def main():\n    value: int64 | str = 1\n    match value:\n        case int64 as number:\n            print(number)\n        case str as text:\n            print(text)\n";

const TWO_UNIONS: &str = "def main():\n    value: int64 | str = 1\n    other: bool | str = true\n    match value:\n        case int64 as number:\n            print(number)\n        case str as text:\n            print(text)\n    match other:\n        case bool as flag:\n            print(flag)\n        case str as text:\n            print(text)\n";

fn local_type(function: &Value, local: &str) -> Value {
    function["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == local)
        .map(|entry| entry["ty"].clone())
        .expect("local should be typed")
}

#[test]
fn union_tag_test_with_a_mismatched_union_type_is_rejected() {
    let mut encoded = encode(TWO_UNIONS);
    let main = function_mut(&mut encoded, "main");
    let other = local_type(main, "other");
    let test = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("UnionTagTest")
            && instruction["Assign"]["value"]["UnionTagTest"]["place"] == json!("value")
    });
    test["Assign"]["value"]["UnionTagTest"]["union_type"] = other;
    assert_rejected(encoded, "does not match place `value` type");
}

#[test]
fn union_tag_test_into_a_non_bool_local_is_rejected() {
    let mut encoded = encode(UNION_MATCH);
    let main = function_mut(&mut encoded, "main");
    let target = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("UnionTagTest")
    })["Assign"]["target"]
        .as_str()
        .unwrap()
        .to_owned();
    retype_local(main, &target, json!({ "Named": ["int64", []] }));
    assert_rejected(encoded, "must assign to a bool local");
}

const OWNED_UNION_MATCH: &str = "def main():\n    value: list[int64] | None = [42]\n    match own value:\n        case list[int64] as values:\n            print(values.len())\n        case None:\n            print(0)\n";

#[test]
fn union_payload_take_into_a_mismatched_destination_is_rejected() {
    let mut encoded = encode(OWNED_UNION_MATCH);
    let main = function_mut(&mut encoded, "main");
    let target = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("UnionTakePayload")
    })["Assign"]["target"]
        .as_str()
        .unwrap()
        .to_owned();
    retype_local(main, &target, json!({ "Named": ["int64", []] }));
    assert_rejected(encoded, "has a destination type mismatch");
}

#[test]
fn union_payload_take_without_an_active_tag_proof_is_rejected() {
    let mut encoded = encode(OWNED_UNION_MATCH);
    let main = function_mut(&mut encoded, "main");
    // Move the take into the entry block ahead of every tag test.
    let take = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("UnionTakePayload")
    })
    .clone();
    let entry_label = main["entry"].clone();
    let entry = find_block(main, |block| block["label"] == entry_label);
    instructions_mut(entry).push(take);
    assert_rejected(encoded, "has no active matching tag proof");
}

#[test]
fn union_payload_taken_twice_is_rejected() {
    let mut encoded = encode(OWNED_UNION_MATCH);
    let main = function_mut(&mut encoded, "main");
    let take = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("UnionTakePayload")
    })
    .clone();
    let target = take["Assign"]["target"].as_str().unwrap().to_owned();
    for block in blocks_mut(main) {
        let instructions = instructions_mut(block);
        if let Some(index) = instructions.iter().position(|instruction| {
            instruction["Assign"]["target"] == target
                && rvalue_kind(instruction) == Some("UnionTakePayload")
        }) {
            let mut second = take.clone();
            second["Assign"]["target"] = json!("%t91");
            instructions.insert(index + 1, second);
        }
    }
    let payload_ty = take["Assign"]["value"]["UnionTakePayload"]["member_type"].clone();
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t91", "ty": payload_ty }));
    assert_rejected(encoded, "already-taken union place");
}

const CLOSURE: &str = "def main():\n    offset = 5\n    add: def(int64) -> int64 = lambda value: value + offset\n    print(add(1))\n";

fn closure_mut(function: &mut Value) -> &mut Value {
    &mut find_instruction(function, |instruction| {
        rvalue_kind(instruction) == Some("Closure")
    })["Assign"]["value"]["Closure"]
}

#[test]
fn closure_with_a_non_function_signature_is_rejected() {
    let mut encoded = encode(CLOSURE);
    closure_mut(function_mut(&mut encoded, "main"))["signature"] = json!("Unit");
    assert_rejected(encoded, "has a non-function signature");
}

#[test]
fn closure_changing_its_declared_contract_is_rejected() {
    let mut encoded = encode(CLOSURE);
    let closure = closure_mut(function_mut(&mut encoded, "main"));
    let signature = closure["signature"].as_object_mut().unwrap();
    let inner = signature.values_mut().next().unwrap();
    inner["params"][0]["ty"] = json!({ "Named": ["str", []] });
    assert_rejected(encoded, "changes its declared callable contract");
}

#[test]
fn value_closure_capture_with_a_source_place_is_rejected() {
    let mut encoded = encode(CLOSURE);
    let closure = closure_mut(function_mut(&mut encoded, "main"));
    closure["captures"][0]["source_place"] = json!("offset");
    assert_rejected(encoded, "has a borrowed source place");
}

#[test]
fn borrowed_closure_capture_that_does_not_read_a_place_is_rejected() {
    let source = "def main():\n    values = [1]\n    count: def() -> int64 = lambda [values]: values.len()\n    print(count())\n";
    let mut encoded = encode(source);
    let closure = closure_mut(function_mut(&mut encoded, "main"));
    closure["captures"][0]["value"] = json!({ "Int": 1 });
    assert_rejected(encoded, "does not read a place");
}

#[test]
fn mutable_closure_capture_escalating_a_shared_loan_is_rejected() {
    let source = "def main():\n    values = [1]\n    view shared = values\n    count: def() -> int64 = lambda [shared]: shared.len()\n    print(count())\n";
    let mut encoded = encode(source);
    let closure = closure_mut(function_mut(&mut encoded, "main"));
    closure["captures"][0]["passing"] = json!("BorrowMut");
    assert_rejected(encoded, "escalates shared loan");
}

#[test]
fn repeatable_closure_whose_body_consumes_a_capture_is_rejected() {
    let source = "def take(text: own str) -> int64:\n    return text.len()\ndef main():\n    text = \"owned\"\n    run: def() -> int64 = lambda [own text]: take(text)\n    print(run())\n";
    let mut encoded = encode(source);
    let closure = closure_mut(function_mut(&mut encoded, "main"));
    closure["consuming"] = json!(false);
    let signature = closure["signature"].as_object_mut().unwrap();
    if let Some(inner) = signature.get_mut("Closure") {
        inner["call_kind"] = json!("Repeatable");
    }
    assert_rejected(encoded, "is consumed by the closure body");
}

const INDIRECT_CALL: &str = "def apply(add: def(int64, int64) -> int64) -> int64:\n    return add(1, 2)\ndef plus(left: int64, right: int64) -> int64:\n    return left + right\ndef main():\n    print(apply(plus))\n";

fn indirect_call_mut(function: &mut Value) -> &mut Value {
    &mut find_instruction(function, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]
                .get("Value")
                .is_some()
    })["Assign"]["value"]["Call"]
}

#[test]
fn indirect_call_with_too_many_positional_arguments_is_rejected() {
    let mut encoded = encode(INDIRECT_CALL);
    let call = indirect_call_mut(function_mut(&mut encoded, "apply"));
    call["args"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": null, "value": { "Int": 3 }, "writeback_place": null }));
    assert_rejected(encoded, "has too many positional arguments");
}

#[test]
fn indirect_call_binding_a_parameter_twice_is_rejected() {
    let source = "def apply(add: def(left: int64, right: int64) -> int64) -> int64:\n    return add(1, right=2)\ndef plus(left: int64, right: int64) -> int64:\n    return left + right\ndef main():\n    print(apply(plus))\n";
    let mut encoded = encode(source);
    let call = indirect_call_mut(function_mut(&mut encoded, "apply"));
    call["args"][1]["name"] = json!("left");
    assert_rejected(encoded, "binds parameter `left` more than once");
}

#[test]
fn indirect_call_omitting_a_required_parameter_is_rejected() {
    let mut encoded = encode(INDIRECT_CALL);
    let call = indirect_call_mut(function_mut(&mut encoded, "apply"));
    call["args"].as_array_mut().unwrap().pop();
    assert_rejected(encoded, "omits required parameter 2");
}

#[test]
fn indirect_call_supplying_writeback_for_a_value_parameter_is_rejected() {
    let mut encoded = encode(INDIRECT_CALL);
    let call = indirect_call_mut(function_mut(&mut encoded, "apply"));
    call["args"][0]["writeback_place"] = json!("add");
    assert_rejected(encoded, "supplies writeback for non-mutable parameter");
}

const MUTABLE_INDIRECT_CALL: &str = "def bump(value: mut int64):\n    value += 1\ndef apply(step: def(mut int64) -> None):\n    mut counter = 1\n    step(counter)\n    print(counter)\ndef main():\n    apply(bump)\n";

#[test]
fn indirect_call_binding_a_mutable_parameter_to_a_non_place_is_rejected() {
    let mut encoded = encode(MUTABLE_INDIRECT_CALL);
    let call = indirect_call_mut(function_mut(&mut encoded, "apply"));
    call["args"][0]["value"] = json!({ "Int": 3 });
    assert_rejected(encoded, "binds mutable parameter");
}

#[test]
fn indirect_call_with_an_inexact_mutable_writeback_is_rejected() {
    let mut encoded = encode(MUTABLE_INDIRECT_CALL);
    let call = indirect_call_mut(function_mut(&mut encoded, "apply"));
    call["args"][0]["writeback_place"] = json!("step");
    assert_rejected(encoded, "requires exact mutable writeback for parameter");
}

#[test]
fn named_call_binding_a_mutable_parameter_to_a_non_place_is_rejected() {
    let source = "def bump(value: mut int64):\n    value += 1\ndef main():\n    mut counter = 1\n    bump(counter)\n    print(counter)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"] == json!({ "Name": "bump" })
    });
    call["Assign"]["value"]["Call"]["args"][0]["value"] = json!({ "MovePlace": "counter" });
    assert_rejected(
        encoded,
        "binds mutable parameter `value` to a non-place operand",
    );
}

#[test]
fn member_call_redirecting_its_receiver_writeback_is_rejected() {
    let source =
        "def main():\n    mut values = [1]\n    values.append(2)\n    print(values.len())\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"]
                == json!("append")
    });
    call["Assign"]["value"]["Call"]["callee"]["Member"]["receiver_place"] = json!("other");
    assert_rejected(
        encoded,
        "redirects receiver `values` to unrelated writeback place `other`",
    );
}

#[test]
fn member_call_binding_a_mutable_receiver_to_a_moved_operand_is_rejected() {
    let source = "class Counter:\n    value: int64\n    def bump(mut self):\n        self.value += 1\ndef main():\n    mut counter = Counter(value=1)\n    counter.bump()\n    print(counter.value)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == json!("bump")
    });
    call["Assign"]["value"]["Call"]["callee"]["Member"]["object"] =
        json!({ "MovePlace": "counter" });
    assert_rejected(encoded, "binds a mutable receiver to a non-place operand");
}

#[test]
fn callable_argument_from_a_non_callable_operand_is_rejected() {
    let source = "def apply(callback: def(int64) -> int64) -> int64:\n    return callback(1)\ndef twice(value: int64) -> int64:\n    return value * 2\ndef main():\n    print(apply(twice))\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"] == json!({ "Name": "apply" })
    });
    call["Assign"]["value"]["Call"]["args"][0]["value"] = json!({ "Int": 1 });
    assert_rejected(
        encoded,
        "passes a non-callable value for a callable contract",
    );
}

#[test]
fn named_argument_to_an_indirect_union_parameter_call_is_validated() {
    let source = "def show(value: int64 | None):\n    match value:\n        case int64 as number:\n            print(number)\n        case None:\n            print(0)\ndef main():\n    handler: def(value: int64 | None) -> None = show\n    handler(value=1)\n    handler(None)\n";
    let mir = lower_source_to_mir(source).expect("source should lower");
    run_mir(&mir).expect("named arguments to union parameters run on the interpreter");
    emit_host_native_object(&mir).expect("named arguments to union parameters emit natively");
}

fn assert_valid(source: &str, label: &str) {
    let mir =
        lower_source_to_mir(source).unwrap_or_else(|error| panic!("{label} should lower: {error}"));
    run_mir(&mir).unwrap_or_else(|error| panic!("{label} should run on the interpreter: {error}"));
    emit_host_native_object(&mir)
        .unwrap_or_else(|error| panic!("{label} should emit natively: {error}"));
}

fn holder_prelude() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef peek(item: Holder):\n    pass\n"
}

#[test]
fn task_result_helpers_carry_callable_identities() {
    let source = format!(
        "{}def make() -> def(own Holder) -> None:\n    return consume\ndef main():\n    with TaskGroup() as group:\n        first = group.start(make)\n        match first.result_or_none(timeout=1s):\n            case Option.Some(callback):\n                callback(Holder(values=[]))\n            case Option.None:\n                print(\"none\")\n        second = group.start(make)\n        fallback: def(own Holder) -> None = second.result_or(consume, timeout=1s)\n        fallback(Holder(values=[1]))\n        third = group.start(make)\n        match third.result(timeout=1s):\n            case TaskResult.Ready(ready):\n                ready(Holder(values=[2]))\n            case _:\n                print(\"not ready\")\n",
        holder_prelude()
    );
    assert_valid(&source, "task result helpers");
}

#[test]
fn map_results_carry_callback_return_identities() {
    let source = format!(
        "{}def main():\n    handlers = [1, 2].map(lambda index: consume)\n    match handlers.get(0):\n        case Option.Some(handler):\n            handler(Holder(values=[]))\n        case Option.None:\n            print(\"none\")\n",
        holder_prelude()
    );
    assert_valid(&source, "map callback identities");
}

#[test]
fn extend_with_nested_identities_and_conditional_append_merge() {
    let source = format!(
        "{}def main():\n    mut pairs: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    more: list[(def(own Holder) -> None, int64)] = [(consume, 1)]\n    pairs.extend(more)\n    match pairs.get(1):\n        case Option.Some(pair):\n            pair[0](Holder(values=[]))\n        case Option.None:\n            print(\"none\")\n    mut callbacks: list[def(own Holder) -> None] = []\n    flag = true\n    if flag:\n        callbacks.append(consume)\n    match callbacks.get(0):\n        case Option.Some(callback):\n            callback(Holder(values=[1]))\n        case Option.None:\n            print(\"none\")\n",
        holder_prelude()
    );
    assert_valid(&source, "extend and conditional append");
}

/// A `peek` function operand whose shared contract differs from `consume`.
fn peek_function_operand(encoded: &Value) -> Value {
    let function = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "peek")
        .expect("peek should be declared");
    json!({ "Function": { "name": "peek", "signature": { "Function": {
        "params": function["params"].as_array().unwrap().iter().map(|param| json!({
            "keyword_only": false, "name": param["name"], "ty": param["ty"], "passing": "Borrow",
            "has_default": false })).collect::<Vec<_>>(),
        "return_type": "Unit" }}}})
}

fn list_literal_mut(function: &mut Value) -> &mut Value {
    &mut find_instruction(function, |instruction| {
        rvalue_kind(instruction) == Some("VecLiteral")
    })["Assign"]["value"]["VecLiteral"]
}

#[test]
fn poisoned_callable_identity_cannot_cross_a_callable_parameter_boundary() {
    let source = format!(
        "{}def invoke(callback: def(own Holder) -> None, value: own Holder):\n    callback(value)\ndef main():\n    callbacks: list[def(own Holder) -> None] = [consume, consume]\n    index = 1\n    match callbacks.get(index):\n        case Option.Some(callback):\n            invoke(callback, Holder(values=[]))\n        case Option.None:\n            print(\"none\")\n",
        holder_prelude()
    );
    let mut encoded = encode(&source);
    let peek = peek_function_operand(&encoded);
    let main = function_mut(&mut encoded, "main");
    list_literal_mut(main)["elements"][1] = peek;
    assert_rejected(
        encoded,
        "passes a non-callable value for a callable contract",
    );
}

#[test]
fn poisoned_nested_callable_identity_cannot_cross_a_tuple_boundary() {
    let source = format!(
        "{}def invoke(pair: (def(own Holder) -> None, int64), value: own Holder):\n    callback: def(own Holder) -> None = pair[0]\n    callback(value)\ndef main():\n    callbacks: list[def(own Holder) -> None] = [consume, consume]\n    index = 1\n    match callbacks.get(index):\n        case Option.Some(callback):\n            invoke((callback, 0), Holder(values=[]))\n        case Option.None:\n            print(\"none\")\n",
        holder_prelude()
    );
    let mut encoded = encode(&source);
    let peek = peek_function_operand(&encoded);
    let main = function_mut(&mut encoded, "main");
    list_literal_mut(main)["elements"][1] = peek;
    assert_rejected(encoded, "no single authoritative contract");
}

#[test]
fn nested_callable_without_an_identity_cannot_cross_a_tuple_boundary() {
    let source = format!(
        "{}def invoke(pair: (def(own Holder) -> None, int64), value: own Holder):\n    callback: def(own Holder) -> None = pair[0]\n    callback(value)\ndef main():\n    pair = (consume, 0)\n    invoke(pair, Holder(values=[]))\n",
        holder_prelude()
    );
    let mut encoded = encode(&source);
    let main = function_mut(&mut encoded, "main");
    let contract = main["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "pair")
        .map(|entry| entry["ty"]["Tuple"][0].clone())
        .expect("pair should be typed");
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t90", "ty": contract }));
    let tuple = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("TupleLiteral")
    });
    tuple["Assign"]["value"]["TupleLiteral"]["elements"][0] = json!({ "Place": "%t90" });
    assert_rejected(encoded, "no authoritative callable contract at");
}

#[test]
fn container_insert_without_an_identity_poisons_the_container() {
    let source = format!(
        "{}def main():\n    mut callbacks: list[def(own Holder) -> None] = [consume]\n    callbacks.append(consume)\n    match callbacks.get(0):\n        case Option.Some(callback):\n            callback(Holder(values=[]))\n        case Option.None:\n            print(\"none\")\n",
        holder_prelude()
    );
    let mut encoded = encode(&source);
    let main = function_mut(&mut encoded, "main");
    let contract = main["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "callbacks")
        .map(|entry| entry["ty"]["Named"][1][0].clone())
        .expect("callbacks should be typed");
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t90", "ty": contract }));
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"]
                == json!("append")
    });
    call["Assign"]["value"]["Call"]["args"][0]["value"] = json!({ "Place": "%t90" });
    assert_rejected(encoded, "authoritative");
}

#[test]
fn container_insert_without_an_operand_poisons_every_element() {
    let source = format!(
        "{}def main():\n    mut callbacks: list[def(own Holder) -> None] = [consume]\n    callbacks.append(consume)\n    match callbacks.get(0):\n        case Option.Some(callback):\n            callback(Holder(values=[]))\n        case Option.None:\n            print(\"none\")\n",
        holder_prelude()
    );
    let mut encoded = encode(&source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"]
                == json!("append")
    });
    call["Assign"]["value"]["Call"]["args"] = json!([]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    run_mir(&mir).expect_err("an append without an operand must not run");
}

const RETURNED_VIEW: &str = "class User:\n    name: str\ndef name(user: User) -> view str from user:\n    return view user.name\ndef main():\n    user = User(name=\"aura\")\n    view current = name(user)\n    print(current.len())\n";

fn return_loan_mut(function: &mut Value) -> &mut Value {
    &mut find_instruction(function, |instruction| {
        instruction.get("ReturnLoan").is_some()
    })["ReturnLoan"]
}

#[test]
fn returned_loan_of_an_arm_local_payload_projection_is_rejected() {
    let mut encoded = encode(RETURNED_VIEW);
    return_loan_mut(function_mut(&mut encoded, "name"))["loan"] = json!("user.__union_payload_0");
    assert_rejected(encoded, "returns an arm-local payload projection");
}

fn begin_returned_loan_mut(function: &mut Value) -> &mut Value {
    &mut find_instruction(function, |instruction| {
        instruction.get("BeginReturnedLoan").is_some()
    })["BeginReturnedLoan"]
}

#[test]
fn returned_loan_descriptor_without_projections_is_rejected() {
    let mut encoded = encode(RETURNED_VIEW);
    begin_returned_loan_mut(function_mut(&mut encoded, "main"))["projections"] = json!([]);
    assert_rejected(encoded, "has no unique inactive descriptor");
}

#[test]
fn returned_loan_descriptor_without_a_preceding_call_is_rejected() {
    let mut encoded = encode(RETURNED_VIEW);
    let main = function_mut(&mut encoded, "main");
    for block in blocks_mut(main) {
        let instructions = instructions_mut(block);
        if let Some(index) = instructions
            .iter()
            .position(|instruction| instruction.get("BeginReturnedLoan").is_some())
        {
            let descriptor = instructions.remove(index);
            instructions.insert(0, descriptor);
        }
    }
    assert_rejected(encoded, "has no immediately preceding call handoff");
}

const BINARY_SOURCE: &str =
    "def main():\n    narrow: int32 = 1\n    wide = 2\n    print(wide + 1)\n";

fn forged_binary(op: &str, left: Value, right: Value) -> MirModule {
    let mut encoded = encode(BINARY_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Binary")
    });
    assign["Assign"]["value"] = json!({ "Binary": { "op": op, "left": left, "right": right, "span": { "line": 4, "column": 11 } } });
    let target = assign["Assign"]["target"].clone();
    // Without a declared result type the interpreter cannot coerce the
    // operands toward the destination, so mismatches surface as errors.
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["name"] != target);
    serde_json::from_value(encoded).expect("forged binary MIR should deserialize")
}

#[test]
fn interpreter_rejects_mismatched_binary_operand_types() {
    for (op, left, right, expected) in [
        (
            "Pow",
            json!({ "Int": 2 }),
            json!({ "Bool": true }),
            "matching numeric operands",
        ),
        (
            "BitAnd",
            json!({ "Int": 2 }),
            json!({ "Float": 1.5 }),
            "matching integer operands",
        ),
        (
            "Shl",
            json!({ "Int": 2 }),
            json!({ "Float": 1.5 }),
            "matching integer operands",
        ),
        (
            "BitOr",
            json!({ "Place": "wide" }),
            json!({ "Place": "narrow" }),
            "operand types must match",
        ),
    ] {
        let error =
            run_mir(&forged_binary(op, left, right)).expect_err("mismatched operands must not run");
        assert!(error.message.contains(expected), "{op}: {}", error.message);
    }
}

#[test]
fn interpreter_rejects_bitwise_not_on_a_non_integer() {
    let mut encoded = encode(BINARY_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Binary")
    });
    assign["Assign"]["value"] = json!({ "Unary": { "op": "BitNot", "value": { "Float": 1.5 }, "span": { "line": 4, "column": 11 } } });
    let target = assign["Assign"]["target"].clone();
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["name"] != target);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("bitwise not on a float must not run");
    assert!(error.message.contains("integer"), "{}", error.message);
}

const UNION_PARAM_HANDLER: &str = "def show(value: int64 | None):\n    match value:\n        case int64 as number:\n            print(number)\n        case None:\n            print(0)\ndef main():\n    handler: def(value: int64 | None) -> None = show\n    handler(1)\n";

#[test]
fn indirect_union_parameter_call_with_too_many_arguments_is_rejected() {
    let mut encoded = encode(UNION_PARAM_HANDLER);
    let call = indirect_call_mut(function_mut(&mut encoded, "main"));
    call["args"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": null, "value": { "Int": 2 }, "writeback_place": null }));
    assert_rejected(encoded, "has too many arguments");
}

#[test]
fn indirect_union_parameter_call_binding_a_parameter_twice_is_rejected() {
    let mut encoded = encode(UNION_PARAM_HANDLER);
    let call = indirect_call_mut(function_mut(&mut encoded, "main"));
    call["args"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "value", "value": { "Int": 2 }, "writeback_place": null }));
    assert_rejected(encoded, "more than once");
}

const TRAIT_MUT_RECEIVER: &str = "trait Counter:\n    def bump(mut self) -> None\nclass Tally:\n    count: int64\nimpl Counter for Tally:\n    def bump(mut self) -> None:\n        self.count += 1\ndef advance[T: Counter](item: mut T):\n    item.bump()\ndef main():\n    mut tally = Tally(count=0)\n    advance(tally)\n    print(tally.count)\n";

#[test]
fn trait_member_call_redirecting_its_receiver_writeback_is_rejected() {
    let mut encoded = encode(TRAIT_MUT_RECEIVER);
    let advance = function_mut(&mut encoded, "advance");
    let call = find_instruction(advance, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]
                .get("TraitMember")
                .is_some()
    });
    call["Assign"]["value"]["Call"]["callee"]["TraitMember"]["receiver_place"] = json!("other");
    assert_rejected(encoded, "redirects receiver");
}

#[test]
fn cleanup_pop_of_a_borrowed_value_is_rejected() {
    let source = "class Resource:\n    values: list[int64]\n    def close(mut self):\n        pass\nclass Holder:\n    resource: Resource\ndef inspect(holder: Holder):\n    print(holder.resource.values.len())\ndef main():\n    pass\n";
    let mut encoded = encode(source);
    let inspect = function_mut(&mut encoded, "inspect");
    inspect["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t90", "ty": { "Named": ["Resource", []] } }));
    let entry_label = inspect["entry"].clone();
    let entry = find_block(inspect, |block| block["label"] == entry_label);
    let instructions = instructions_mut(entry);
    instructions.insert(0, json!({ "Assign": { "target": "%t90", "value": { "Use": { "Place": "holder.resource" } } } }));
    instructions.insert(
        1,
        json!({ "PopCleanup": { "place": "%t90", "cancel_before_cleanup": false } }),
    );
    assert_rejected(encoded, "cannot close a borrowed value");
}

#[test]
fn union_payload_take_from_a_different_union_with_the_same_member_index_is_rejected() {
    let source = "def main():\n    value: list[int64] | None = [42]\n    other: list[int64] | str = [1]\n    match own value:\n        case list[int64] as values:\n            print(values.len())\n        case None:\n            print(0)\n    match other:\n        case list[int64] as items:\n            print(items.len())\n        case str as text:\n            print(text)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let other = local_type(main, "other");
    let take = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("UnionTakePayload")
    });
    let index = take["Assign"]["value"]["UnionTakePayload"]["member_index"].clone();
    let other_members = other["Union"]["members"].clone();
    if other_members[index.as_u64().unwrap() as usize]
        != take["Assign"]["value"]["UnionTakePayload"]["member_type"]
    {
        // Canonical ordering placed the shared member elsewhere; the earlier
        // member-index check owns this shape.
        return;
    }
    take["Assign"]["value"]["UnionTakePayload"]["union_type"] = other;
    assert_rejected(encoded, "does not match place");
}

#[test]
fn variant_payload_from_a_tuple_typed_scrutinee_is_rejected() {
    let source = "enum Shape:\n    Circle(int64)\n    Square(int64)\ndef main():\n    pair = (1, 2)\n    shape = Shape.Circle(3)\n    match shape:\n        case Shape.Circle(radius):\n            print(radius)\n        case Shape.Square(side):\n            print(side)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("VariantPayload")
    });
    assign["Assign"]["value"]["VariantPayload"]["scrutinee"] = json!({ "Place": "pair" });
    assert_rejected(encoded, "requires an enum scrutinee");
}

#[test]
fn callable_argument_without_an_identity_is_rejected() {
    let source = "def apply(callback: def(int64) -> int64) -> int64:\n    return callback(1)\ndef twice(value: int64) -> int64:\n    return value * 2\ndef main():\n    print(apply(twice))\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let contract = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "apply")
        .map(|function| function["params"][0]["ty"].clone())
        .unwrap();
    let main = function_mut(&mut encoded, "main");
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t90", "ty": contract }));
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"] == json!({ "Name": "apply" })
    });
    call["Assign"]["value"]["Call"]["args"][0]["value"] = json!({ "Place": "%t90" });
    assert_rejected(encoded, "has no authoritative callable contract for `%t90`");
}

#[test]
fn task_start_of_a_callable_without_an_identity_is_rejected() {
    let source = "def worker() -> int64:\n    return 1\ndef main():\n    with TaskGroup() as group:\n        task = group.start(worker)\n        print(task.result_or(0, timeout=1s))\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let start = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("StartTask")
    });
    let signature =
        start["Assign"]["value"]["StartTask"]["function"]["Function"]["signature"].clone();
    start["Assign"]["value"]["StartTask"]["function"] = json!({ "Place": "%t90" });
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t90", "ty": signature }));
    assert_rejected(encoded, "has no authoritative callable contract");
}

#[test]
fn closure_capture_renamed_from_its_declaration_is_rejected() {
    let source = "def main():\n    offset = 5\n    add: def(int64) -> int64 = lambda value: value + offset\n    print(add(1))\n";
    let mut encoded = encode(source);
    let closure = closure_mut(function_mut(&mut encoded, "main"));
    closure["captures"][0]["name"] = json!("renamed");
    assert_rejected(encoded, "changes its declared callable contract");
}

const RETURNED_VIEW_WITH_MATCH: &str = "class Holder:\n    value: int64 | str\n    score: int64\ndef pick(holder: Holder) -> view int64 from holder:\n    match holder.value:\n        case int64 as number:\n            print(number)\n        case str as text:\n            print(text)\n    return view holder.score\ndef main():\n    holder = Holder(value=1, score=2)\n    view picked = pick(holder)\n    print(picked)\n";

#[test]
fn returned_loan_of_an_arm_pattern_loan_is_rejected() {
    let mut encoded = encode(RETURNED_VIEW_WITH_MATCH);
    let pick = function_mut(&mut encoded, "pick");
    let pattern_loan = find_instruction(pick, |instruction| {
        instruction["BeginLoan"]["source"]
            .as_str()
            .is_some_and(|source| source.contains("__union_payload_"))
    })["BeginLoan"]["loan"]
        .clone();
    return_loan_mut(pick)["loan"] = pattern_loan;
    assert_rejected(encoded, "returns an arm-local payload projection");
}

#[test]
fn returned_loan_transferred_without_returning_is_rejected() {
    let mut encoded = encode(RETURNED_VIEW_WITH_MATCH);
    let pick = function_mut(&mut encoded, "pick");
    let mut return_loan = None;
    for block in blocks_mut(pick) {
        let instructions = instructions_mut(block);
        if let Some(index) = instructions
            .iter()
            .position(|instruction| instruction.get("ReturnLoan").is_some())
        {
            return_loan = Some(instructions.remove(index));
        }
    }
    let return_loan = return_loan.expect("the returned-view function should hand off a loan");
    let entry_label = pick["entry"].clone();
    let entry = find_block(pick, |block| block["label"] == entry_label);
    instructions_mut(entry).push(return_loan);
    assert_rejected(encoded, "transfers a returned loan without returning");
}

#[test]
fn map_over_callable_elements_carries_callback_return_identities() {
    let source = format!(
        "{}def main():\n    callbacks: list[def(own Holder) -> None] = [consume]\n    mapped = callbacks.map(lambda callback: consume)\n    match mapped.get(0):\n        case Option.Some(handler):\n            handler(Holder(values=[]))\n        case Option.None:\n            print(\"none\")\n",
        holder_prelude()
    );
    assert_valid(&source, "map over callable elements");
}

fn assert_native_rejects(encoded: Value, expected: &str) {
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert!(
        native.contains(expected),
        "native rejection `{native}` should mention `{expected}`"
    );
}

fn terminator_mut<'a>(function: &'a mut Value, kind: &str) -> &'a mut Value {
    let block = find_block(function, |block| block["terminator"].get(kind).is_some());
    &mut block["terminator"][kind]
}

const ASSERTION: &str =
    "def main():\n    value = 1\n    assert value == 1, \"message\"\n    print(value)\n";

#[test]
fn native_backend_rejects_a_non_string_assertion_message() {
    let mut encoded = encode(ASSERTION);
    let main = function_mut(&mut encoded, "main");
    terminator_mut(main, "AssertFail")["message"] = json!({ "Int": 1 });
    assert_native_rejects(encoded, "expected an assertion message to be `str`");
}

#[test]
fn native_backend_rejects_a_single_assertion_capture() {
    let mut encoded = encode(ASSERTION);
    let main = function_mut(&mut encoded, "main");
    let assert_fail = terminator_mut(main, "AssertFail");
    let captures = assert_fail["captures"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut single = captures
        .first()
        .cloned()
        .map(|capture| vec![capture])
        .unwrap_or_else(|| vec![json!({ "label": "value", "value": { "Place": "value" } })]);
    if single.len() != 1 {
        single.truncate(1);
    }
    assert_fail["captures"] = json!(single);
    assert_native_rejects(encoded, "exactly two assertion captures");
}

#[test]
fn native_backend_rejects_unsupported_unary_operand_types() {
    let mut encoded = encode(BINARY_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Binary")
    });
    assign["Assign"]["value"] = json!({ "Unary": { "op": "BitNot", "value": { "Float": 1.5 }, "span": { "line": 4, "column": 11 } } });
    assert_native_rejects(encoded, "unsupported value coercion");
}

#[test]
fn native_backend_rejects_unsupported_unary_operations_on_untyped_targets() {
    let mut encoded = encode(BINARY_SOURCE);
    let main = function_mut(&mut encoded, "main");
    let assign = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Binary")
    });
    assign["Assign"]["value"] = json!({ "Unary": { "op": "BitNot", "value": { "Float": 1.5 }, "span": { "line": 4, "column": 11 } } });
    let target = assign["Assign"]["target"].clone();
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["name"] != target);
    assert_native_rejects(encoded, "could not infer direct type");
}

#[test]
fn native_backend_rejects_sqrt_with_arguments() {
    let source = "def main():\n    value = 2.0\n    print(value.sqrt())\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == json!("sqrt")
    });
    call["Assign"]["value"]["Call"]["args"] =
        json!([{ "name": null, "value": { "Float": 1.0 }, "writeback_place": null }]);
    assert_native_rejects(encoded, "expected `sqrt()` to take no arguments");
}

#[test]
fn native_backend_rejects_yield_now_with_arguments() {
    let source = "def main():\n    yield_now()\n    print(1)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"] == json!({ "Name": "yield_now" })
    });
    call["Assign"]["value"]["Call"]["args"] =
        json!([{ "name": null, "value": { "Int": 1 }, "writeback_place": null }]);
    assert_native_rejects(encoded, "expected `yield_now()` to take no arguments");
}

#[test]
fn native_backend_rejects_array_map_with_a_non_callable_callback() {
    let source = "def main():\n    values = Array[float64].from_list([1.0, 2.0], [2])\n    print(values.map(lambda value: value * 2.0).sum())\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let call = find_instruction(main, |instruction| {
        rvalue_kind(instruction) == Some("Call")
            && instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == json!("map")
    });
    call["Assign"]["value"]["Call"]["args"][0]["value"] = json!({ "Int": 1 });
    assert_native_rejects(encoded, "Array.map");
}
