//! Forged-MIR regressions for class initializers (ADR-0059). A class with
//! `__init__` lowers to a receiver-less `Class.__init__` function that builds
//! its object in an owned `self` local whose undefaulted fields start absent.
//! Each case lowers a valid program, corrupts one MIR fact, and asserts that
//! the interpreter and the direct backend both refuse the module for the
//! shared validator's reason.

use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

const SOURCE: &str = "class Pair:
    left: int64
    right: list[int64]
    label: str = \"pair\"

    def __init__(self, value: int64):
        self.left = value
        if value > 0:
            self.right = [value]
        else:
            self.right = []
        print(self.label)

    def total(self) -> int64:
        return self.left + self.right.len()

def main():
    pair = Pair(2)
    print(pair.total())
";

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

fn is_assign_to(instruction: &Value, target: &str) -> bool {
    instruction
        .get("Assign")
        .is_some_and(|assign| assign["target"] == target)
}

fn assert_rejected(encoded: Value, expected: &str) {
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

#[test]
fn source_initializer_validates_and_runs() {
    let mir = lower_source_to_mir(SOURCE).expect("source should lower");
    let initializer = mir
        .functions
        .iter()
        .find(|function| function.name == "Pair.__init__")
        .expect("the initializer lowers to its own function");
    assert!(initializer.receiver.is_none());
    let output = run_mir(&mir).expect("source-produced initializer should run");
    assert_eq!(output.stdout, "pair\n3\n");
    emit_host_native_object(&mir).expect("source-produced initializer should compile");
}

#[test]
fn partial_construction_outside_an_initializer_is_rejected() {
    let mut encoded = encode(SOURCE);
    let main = function_mut(&mut encoded, "main");
    let entry = &mut blocks_mut(main)[0];
    instructions_mut(entry).insert(
        0,
        json!({"Assign": {"target": "forged", "value": {"Construct": {"class_name": "Pair", "fields": []}}}}),
    );
    function_mut(&mut encoded, "main")["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "forged", "ty": {"Named": ["Pair", []]}}));
    assert_rejected(
        encoded,
        "invalid MIR construction of `Pair` in `main` omits fields outside an initializer's object",
    );
}

#[test]
fn second_initializer_slot_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    let entry = &mut blocks_mut(initializer)[0];
    let slot = instructions_mut(entry)
        .iter()
        .find(|instruction| is_assign_to(instruction, "self"))
        .cloned()
        .expect("the initializer allocates its object");
    instructions_mut(entry).push(slot);
    assert_rejected(encoded, "omits fields outside an initializer's object");
}

#[test]
fn reading_an_absent_field_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    let entry = &mut blocks_mut(initializer)[0];
    let slot_index = instructions_mut(entry)
        .iter()
        .position(|instruction| is_assign_to(instruction, "self"))
        .expect("the initializer allocates its object");
    instructions_mut(entry).insert(
        slot_index + 1,
        json!({"Assign": {"target": "value", "value": {"Member": {"object": {"Place": "self"}, "field": "left"}}}}),
    );
    // A member read of the whole object names `self`, which is incomplete.
    assert_rejected(
        encoded,
        "invalid MIR initializer `Pair.__init__` uses `self` before every field is initialized",
    );
}

#[test]
fn lending_an_absent_field_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    let entry = &mut blocks_mut(initializer)[0];
    let slot_index = instructions_mut(entry)
        .iter()
        .position(|instruction| is_assign_to(instruction, "self"))
        .expect("the initializer allocates its object");
    instructions_mut(entry).insert(
        slot_index + 1,
        json!({"BeginLoan": {"loan": "early", "source": "self.right", "mutable": false}}),
    );
    assert_rejected(
        encoded,
        "invalid MIR initializer `Pair.__init__` reads field `right` of `self` before initializing it",
    );
}

#[test]
fn returning_an_incomplete_object_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    // Drop the else-branch initialization of `right`: one path now returns
    // the object without it.
    for block in blocks_mut(initializer) {
        let label = block["label"].as_str().unwrap().to_owned();
        if label.contains("else") {
            instructions_mut(block).retain(|instruction| !is_assign_to(instruction, "self.right"));
        }
    }
    assert_rejected(
        encoded,
        "invalid MIR initializer `Pair.__init__` uses `self` before every field is initialized",
    );
}

#[test]
fn using_self_before_allocation_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    let entry = &mut blocks_mut(initializer)[0];
    instructions_mut(entry).insert(0, json!({"Eval": {"value": {"Place": "self.label"}}}));
    assert_rejected(
        encoded,
        "invalid MIR initializer `Pair.__init__` uses `self` before allocating it",
    );
}

#[test]
fn nested_write_into_an_absent_field_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    let entry = &mut blocks_mut(initializer)[0];
    let slot_index = instructions_mut(entry)
        .iter()
        .position(|instruction| is_assign_to(instruction, "self"))
        .expect("the initializer allocates its object");
    instructions_mut(entry).insert(
        slot_index + 1,
        json!({"Assign": {"target": "self.right.0", "value": {"Use": {"Int": 1}}}}),
    );
    assert_rejected(
        encoded,
        "reads field `right` of `self` before initializing it",
    );
}

#[test]
fn re_entering_the_allocating_block_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    let entry = initializer["entry"].clone();
    for block in blocks_mut(initializer) {
        if block["terminator"].get("Return").is_some() {
            block["terminator"] = json!({"Goto": entry});
        }
    }
    assert_rejected(encoded, "re-enters the block that allocates its object");
}

#[test]
fn jumping_to_an_unknown_block_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    for block in blocks_mut(initializer) {
        if block["terminator"].get("Return").is_some() {
            block["terminator"] = json!({"Goto": "nowhere"});
        }
    }
    assert_rejected(encoded, "nowhere");
}

#[test]
fn partial_construction_into_another_local_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    for block in blocks_mut(initializer) {
        for instruction in instructions_mut(block) {
            if is_assign_to(instruction, "self") {
                instruction["Assign"]["target"] = json!("other");
            }
        }
    }
    assert_rejected(encoded, "omits fields outside an initializer's object");
}

#[test]
fn partial_construction_in_a_receiver_method_is_rejected() {
    let mut encoded = encode(SOURCE);
    let initializer = function_mut(&mut encoded, "Pair.__init__");
    initializer["receiver"] = json!("BorrowMut");
    assert_rejected(encoded, "omits fields outside an initializer's object");
}
