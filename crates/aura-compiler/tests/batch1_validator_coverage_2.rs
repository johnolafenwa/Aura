//! Forged-MIR regressions for validator place typing and metadata lookups.

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

fn instruction_mut<'a>(function: &'a mut Value, matches: impl Fn(&Value) -> bool) -> &'a mut Value {
    for block in function["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if matches(instruction) {
                return instruction;
            }
        }
    }
    panic!("expected instruction not found");
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

fn assigned_target(instruction: &Value) -> Option<&str> {
    instruction["Assign"]["target"].as_str()
}

const PLACES: &str = "class Holder:\n    count: int64\ndef main():\n    holder = Holder(count=1)\n    pair = (1, 2)\n    copy = holder.count\n    print(copy)\n";

#[test]
fn tuple_projection_without_a_fixed_position_is_rejected() {
    let mut encoded = encode(PLACES);
    let main = function_mut(&mut encoded, "main");
    instruction_mut(main, |instruction| {
        assigned_target(instruction) == Some("copy")
    })["Assign"]["value"] = json!({ "Use": { "Place": "pair.first" } });
    assert_rejected(encoded, "is not a fixed position");
}

#[test]
fn tuple_projection_out_of_bounds_is_rejected() {
    let mut encoded = encode(PLACES);
    let main = function_mut(&mut encoded, "main");
    instruction_mut(main, |instruction| {
        assigned_target(instruction) == Some("copy")
    })["Assign"]["value"] = json!({ "Use": { "Place": "pair.7" } });
    assert_rejected(encoded, "7");
}

#[test]
fn class_projection_naming_a_missing_field_is_rejected() {
    let mut encoded = encode(PLACES);
    let main = function_mut(&mut encoded, "main");
    instruction_mut(main, |instruction| {
        assigned_target(instruction) == Some("copy")
    })["Assign"]["value"] = json!({ "Use": { "Place": "holder.missing" } });
    assert_rejected(encoded, "field `missing`");
}

const METHODS: &str = "class Counter:\n    value: int64\n    def bump(mut self):\n        self.value += 1\ndef main():\n    mut counter = Counter(value=1)\n    counter.bump()\n    print(counter.value)\n";

#[test]
fn class_method_referencing_a_missing_function_is_rejected() {
    let mut encoded = encode(METHODS);
    let class = encoded["classes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|class| class["name"] == "Counter")
        .expect("Counter should be declared");
    class["methods"][0]["function_name"] = json!("Counter::vanished");
    assert_rejected(encoded, "references missing function `Counter::vanished`");
}

const TRAIT_METHODS: &str = "trait Counter:\n    def bump(mut self) -> None\nclass Tally:\n    count: int64\nimpl Counter for Tally:\n    def bump(mut self) -> None:\n        self.count += 1\ndef advance[T: Counter](item: mut T):\n    item.bump()\ndef main():\n    mut tally = Tally(count=0)\n    advance(tally)\n    print(tally.count)\n";

#[test]
fn trait_method_referencing_a_missing_function_is_rejected() {
    let mut encoded = encode(TRAIT_METHODS);
    let implementation = encoded["trait_impls"]
        .as_array_mut()
        .unwrap()
        .first_mut()
        .expect("the trait implementation should be declared");
    let methods = implementation["methods"].as_object_mut().or_else(|| None);
    match methods {
        Some(methods) => {
            for (_, function_name) in methods.iter_mut() {
                *function_name = json!("Tally::vanished");
            }
        }
        None => {
            for method in implementation["methods"].as_array_mut().unwrap() {
                method["function_name"] = json!("Tally::vanished");
            }
        }
    }
    assert_rejected(encoded, "references missing function");
}

const RETURNED_VIEW_METHOD: &str = "class User:\n    name: str\n    def title(self) -> view str from self:\n        return view self.name\ndef main():\n    user = User(name=\"aura\")\n    view current = user.title()\n    print(current.len())\n";

#[test]
fn returned_view_method_call_without_an_addressable_receiver_is_rejected() {
    let mut encoded = encode(RETURNED_VIEW_METHOD);
    let main = function_mut(&mut encoded, "main");
    let call = instruction_mut(main, |instruction| {
        instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == json!("title")
    });
    call["Assign"]["value"]["Call"]["callee"]["Member"]["object"] = json!({ "MovePlace": "user" });
    call["Assign"]["value"]["Call"]["callee"]["Member"]["receiver_place"] = Value::Null;
    assert_rejected(encoded, "receiver");
}

#[test]
fn serialized_runtime_failures_render_as_diagnostics() {
    let source = "def divide(left: int64, right: int64) -> int64:\n    return left // right\ndef main():\n    print(divide(1, 0))\n";
    let mir = lower_source_to_mir(source).expect("source should lower");
    let serialized = serde_json::to_vec(&mir).expect("MIR should serialize");
    let error = aura_compiler::run_serialized_mir(&serialized, "<forged>", source)
        .expect_err("integer division by zero must fail at runtime");
    assert!(error.message.contains("zero"), "{}", error.message);
    let interpreted = run_mir(&mir).expect_err("the in-memory runtime reports the same failure");
    assert!(
        interpreted.message.contains("zero"),
        "{}",
        interpreted.message
    );
}

#[test]
fn consuming_operands_in_non_consuming_positions_fail_at_runtime() {
    let source = "def main():\n    flag = true\n    if flag:\n        print(1)\n    else:\n        print(2)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let block = main["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["terminator"].get("Branch").is_some())
        .expect("the if lowers to a branch");
    let condition = block["terminator"]["Branch"]["condition"].clone();
    if let Some(place) = condition.get("Place").cloned() {
        block["terminator"]["Branch"]["condition"] = json!({ "MovePlace": place });
    }
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let error = run_mir(&mir).expect_err("a consumed branch condition must not evaluate");
    assert!(error.message.contains("non-consuming"), "{}", error.message);
}

const BYTES_CALLS: &str = "import bytes\ndef main():\n    text = \"aura\"\n    binary = text.to_bytes()\n    print(bytes.base64_encode(binary))\n    print(bytes.sha256_string(text).len())\n    mut values = [1, 2]\n    values.reserve(4)\n    print(values.len())\n";

fn forge_named_call_argument(name_fragment: &str, replacement: Value) -> MirModule {
    let mut encoded = encode(BYTES_CALLS);
    let main = function_mut(&mut encoded, "main");
    let call = instruction_mut(main, |instruction| {
        instruction["Assign"]["value"]["Call"]["callee"]["Name"]
            .as_str()
            .is_some_and(|name| name.contains(name_fragment))
    });
    call["Assign"]["value"]["Call"]["args"][0]["value"] = replacement;
    serde_json::from_value(encoded).expect("forged MIR should deserialize")
}

#[test]
fn bytes_builtins_reject_mistyped_runtime_arguments() {
    let error = run_mir(&forge_named_call_argument(
        "base64_encode",
        json!({ "Int": 1 }),
    ))
    .expect_err("base64_encode must reject a non-bytes runtime value");
    assert!(error.message.contains("list[uint8]"), "{}", error.message);
    let error = run_mir(&forge_named_call_argument(
        "sha256_string",
        json!({ "Int": 1 }),
    ))
    .expect_err("sha256_string must reject a non-string runtime value");
    assert!(error.message.contains("str"), "{}", error.message);
}

#[test]
fn list_reserve_rejects_a_non_integer_runtime_count() {
    let mut encoded = encode(BYTES_CALLS);
    let main = function_mut(&mut encoded, "main");
    let call = instruction_mut(main, |instruction| {
        instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == json!("reserve")
    });
    call["Assign"]["value"]["Call"]["args"][0]["value"] = json!({ "Float": 1.5 });
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let error = run_mir(&mir).expect_err("reserve must reject a non-integer runtime count");
    assert!(error.message.contains("int64"), "{}", error.message);
}

#[test]
fn divmod_infers_its_operand_type_from_untyped_runtime_values() {
    let source = "def main():\n    print(divmod(7, 2))\n    print(divmod(7.5, 2.0))\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let mut targets = Vec::new();
    for block in main["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            let is_divmod = instruction["Assign"]["value"]["Call"]["callee"]["Name"]
                .as_str()
                .is_some_and(|name| name.ends_with("divmod"));
            if !is_divmod {
                continue;
            }
            targets.push(instruction["Assign"]["target"].as_str().unwrap().to_owned());
            for arg in instruction["Assign"]["value"]["Call"]["args"]
                .as_array()
                .unwrap()
            {
                if let Some(place) = arg["value"]["Place"].as_str() {
                    targets.push(place.to_owned());
                }
            }
        }
    }
    assert!(!targets.is_empty(), "divmod calls should be present");
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| !targets.iter().any(|target| entry["name"] == *target));
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let output = run_mir(&mir).expect("divmod infers operand types from the runtime values");
    assert_eq!(output.stdout, "(3, 1)\n(3.0, 1.5)\n");
}

#[test]
fn list_indices_outside_the_signed_range_fail_at_runtime() {
    let source = "def main():\n    values = [1, 2, 3]\n    index = 1\n    print(values[index])\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    instruction_mut(main, |instruction| {
        assigned_target(instruction) == Some("index")
    })["Assign"]["value"] = json!({ "Use": { "Int": 9223372036854775807i64 } });
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let error = run_mir(&mir).expect_err("an index beyond the list must fail");
    assert!(
        error.message.contains("index") || error.message.contains("range"),
        "{}",
        error.message
    );
}

#[test]
fn union_payload_moves_require_a_checked_take_at_runtime() {
    let source = "def main():\n    value: list[int64] | None = [42]\n    match own value:\n        case list[int64] as values:\n            print(values.len())\n        case None:\n            print(0)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let take = instruction_mut(main, |instruction| {
        instruction["Assign"]["value"]
            .get("UnionTakePayload")
            .is_some()
    });
    let place = take["Assign"]["value"]["UnionTakePayload"]["place"]
        .as_str()
        .unwrap()
        .to_owned();
    let index = take["Assign"]["value"]["UnionTakePayload"]["member_index"]
        .as_u64()
        .unwrap();
    take["Assign"]["value"] =
        json!({ "Use": { "MovePlace": format!("{place}.__union_payload_{index}") } });
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let error = run_mir(&mir).expect_err("a raw payload move must be rejected");
    assert!(
        error.message.contains("UnionTakePayload") || error.message.contains("payload"),
        "{}",
        error.message
    );
}

const FORMATTED: &str =
    "def main():\n    value = 42\n    text = \"aura\"\n    print(f\"{value:x} {text:>6}\")\n";

fn forge_format_spec(mutate: impl Fn(&mut Value)) -> MirModule {
    let mut encoded = encode(FORMATTED);
    let main = function_mut(&mut encoded, "main");
    let format = instruction_mut(main, |instruction| {
        instruction["Assign"]["value"].get("FormatString").is_some()
    });
    for part in format["Assign"]["value"]["FormatString"]["parts"]
        .as_array_mut()
        .unwrap()
    {
        if part.get("Formatted").is_some() {
            mutate(&mut part["Formatted"]);
        }
    }
    serde_json::from_value(encoded).expect("forged MIR should deserialize")
}

#[test]
fn runtime_format_codes_must_match_the_value_kind() {
    let mir = forge_format_spec(|part| {
        if part["value_type"] == json!({ "Named": ["str", []] }) {
            part["spec"] = json!("d");
        }
    });
    let error = run_mir(&mir).expect_err("an integer code on a string must fail at runtime");
    assert!(error.message.contains("format"), "{}", error.message);
    let mir = forge_format_spec(|part| {
        if part["value_type"] == json!({ "Named": ["int64", []] }) {
            part["spec"] = json!("s");
        }
    });
    let error = run_mir(&mir).expect_err("a string code on an integer must fail at runtime");
    assert!(
        error.message.contains("str") || error.message.contains("format"),
        "{}",
        error.message
    );
}

#[test]
fn runtime_format_widths_are_bounded() {
    let mir = forge_format_spec(|part| {
        if part["value_type"] == json!({ "Named": ["str", []] }) {
            part["spec"] = json!(">99999999999999");
        }
    });
    let error = run_mir(&mir).expect_err("an enormous width must fail at runtime");
    assert!(
        error.message.contains("width") || error.message.contains("allocation"),
        "{}",
        error.message
    );
    let mir = forge_format_spec(|part| {
        if part["value_type"] == json!({ "Named": ["int64", []] }) {
            part["spec"] = json!("^99999999999999");
        }
    });
    let error = run_mir(&mir).expect_err("an enormous centered width must fail at runtime");
    assert!(
        error.message.contains("width") || error.message.contains("allocation"),
        "{}",
        error.message
    );
}
