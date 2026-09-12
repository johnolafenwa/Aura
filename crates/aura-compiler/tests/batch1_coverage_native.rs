//! Direct-backend (Cranelift) coverage: codegen paths reached through
//! `emit_host_native_object`, and the backend's defensive rejections of
//! forged MIR that the shared validator does not refuse. Mutations the
//! shared validator does refuse are asserted on both public boundaries so
//! the file never records a backend-only containment as the whole story.

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

/// Every object-shaped instruction; unit instruction variants serialize as
/// bare strings and never carry forgeable payloads.
fn instructions_mut(function: &mut Value) -> impl Iterator<Item = &mut Value> {
    function["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
        .filter(|instruction| instruction.is_object())
}

fn member_call_mut<'a>(function: &'a mut Value, field: &str) -> &'a mut Value {
    let field = json!(field);
    instructions_mut(function)
        .find(|instruction| {
            instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == field
        })
        .map(|instruction| &mut instruction["Assign"]["value"]["Call"])
        .unwrap_or_else(|| panic!("member call `{field}` not found"))
}

fn named_call_mut<'a>(function: &'a mut Value, name: &str) -> &'a mut Value {
    instructions_mut(function)
        .find(|instruction| {
            instruction["Assign"]["value"]["Call"]["callee"]["Name"]
                .as_str()
                .is_some_and(|candidate| {
                    candidate == name || candidate.ends_with(&format!("::{name}"))
                })
        })
        .map(|instruction| &mut instruction["Assign"]["value"]["Call"])
        .unwrap_or_else(|| panic!("named call `{name}` not found"))
}

fn assign_mut<'a>(function: &'a mut Value, target: &str) -> &'a mut Value {
    instructions_mut(function)
        .find(|instruction| instruction["Assign"]["target"] == json!(target))
        .map(|instruction| &mut instruction["Assign"]["value"])
        .unwrap_or_else(|| panic!("assignment to `{target}` not found"))
}

fn local_type_mut<'a>(function: &'a mut Value, name: &str) -> &'a mut Value {
    function["local_types"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|local| local["name"] == name)
        .map(|local| &mut local["ty"])
        .unwrap_or_else(|| panic!("local `{name}` should have a declared type"))
}

/// Reads a nested key path without inserting keys the way `IndexMut` does.
fn path<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().try_fold(value, |value, key| value.get(key))
}

fn member_field(instruction: &Value) -> Option<&str> {
    path(
        instruction,
        &["Assign", "value", "Call", "callee", "Member", "field"],
    )
    .and_then(Value::as_str)
}

fn member_has_receiver_place(instruction: &Value) -> bool {
    path(
        instruction,
        &[
            "Assign",
            "value",
            "Call",
            "callee",
            "Member",
            "receiver_place",
        ],
    )
    .is_some_and(Value::is_string)
}

fn returned_view_type(pointee: Value) -> Value {
    json!({ "ReturnedView": { "mutable": false, "pointee": pointee, "origin": 0 } })
}

fn decode(encoded: Value) -> MirModule {
    serde_json::from_value(encoded).expect("forged MIR should deserialize")
}

fn native_rejection(encoded: Value, expected: &str) -> String {
    match emit_host_native_object(&decode(encoded)) {
        Ok(_) => panic!("native emission must reject the forged module (expected `{expected}`)"),
        Err(error) => error,
    }
}

fn assert_native_rejects(encoded: Value, expected: &str) {
    let native = native_rejection(encoded, expected);
    assert!(
        native.contains(expected),
        "native rejection `{native}` should mention `{expected}`"
    );
}

fn assert_native_emits(encoded: Value) {
    let object = emit_host_native_object(&decode(encoded))
        .expect("the direct backend should emit an object for this module");
    assert!(!object.is_empty());
}

/// Rejections owned by the shared validator surface identically on both
/// public boundaries: the interpreter prefixes the validator's message with
/// `invalid MIR loan flow: ` and the direct backend reports it bare.
fn assert_rejected_on_both_boundaries(encoded: Value, expected: &str) {
    let mir = decode(encoded);
    let interpreted = run_mir(&mir).expect_err("the interpreter must reject the forged module");
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

/// A mutation both boundaries accept: the interpreter's stdout is pinned and
/// the direct backend must emit an object for the same module.
fn assert_accepted_on_both_boundaries(encoded: Value, expected_stdout: &str) {
    let mir = decode(encoded);
    let output = run_mir(&mir).expect("the interpreter should accept this module");
    assert_eq!(output.stdout, expected_stdout);
    let object = emit_host_native_object(&mir)
        .expect("the direct backend should emit an object for this module");
    assert!(!object.is_empty());
}

fn assert_source_emits(source: &str) {
    assert_native_emits(encode(source));
}

// ---------------------------------------------------------------------------
// Arrays
// ---------------------------------------------------------------------------

const ARRAYS: &str = "def main():\n    values = Array[float64].from_list([1.0, 2.0], [2])\n    doubled = values + values\n    scaled = 2.0 * values\n    print(doubled.sum())\n    print(scaled.len())\n";

#[test]
fn native_backend_rejects_unsupported_array_dtypes_and_operators() {
    assert_source_emits(ARRAYS);

    // The shared validator's named-builtin contract table refuses a forged
    // constructor result type before the backend's own dtype check runs.
    let mut encoded = encode(ARRAYS);
    let main = function_mut(&mut encoded, "main");
    *local_type_mut(main, "%t0") = json!({ "Named": ["Array", [{ "Named": ["str", []] }]] });
    assert_native_rejects(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` requires an `Array` result type with a numeric dtype",
    );

    let mut encoded = encode(ARRAYS);
    let main = function_mut(&mut encoded, "main");
    *local_type_mut(main, "%t0") = json!({ "Named": ["int64", []] });
    assert_native_rejects(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` requires an `Array` result type with a numeric dtype",
    );

    let mut encoded = encode(ARRAYS);
    let main = function_mut(&mut encoded, "main");
    let mut forged = false;
    for instruction in instructions_mut(main) {
        if instruction["Assign"]["value"]["Binary"]["op"] == json!("Add") {
            instruction["Assign"]["value"]["Binary"]["op"] = json!("Mod");
            forged = true;
            break;
        }
    }
    assert!(forged, "the array sum should lower to a binary `Add`");
    assert_native_rejects(encoded, "does not support Array binary operation `Mod`");
}

// ---------------------------------------------------------------------------
// Indirect calls through function values
// ---------------------------------------------------------------------------

const INDIRECT: &str = "def add(left: int64, right: int64) -> int64:\n    return left + right\ndef apply(function: def(int64, int64) -> int64, left: int64, right: int64) -> int64:\n    return function(left, right)\ndef main():\n    print(apply(add, 1, 2))\n";

#[test]
fn native_backend_lowers_indirect_calls_through_function_values() {
    assert_source_emits(INDIRECT);

    let mut encoded = encode(INDIRECT);
    let apply = function_mut(&mut encoded, "apply");
    let call = instructions_mut(apply)
        .find(|instruction| {
            path(instruction, &["Assign", "value", "Call", "callee", "Value"]).is_some()
        })
        .map(|instruction| &mut instruction["Assign"]["value"]["Call"])
        .expect("apply should call its function value");
    call["args"][0]["value"] = json!({ "Place": "missing_operand" });
    // The shared validator does not resolve indirect-call operand places, so
    // each boundary refuses the unknown place on its own: the direct backend
    // when it lowers the call, the interpreter when the call executes. Neither
    // may lower or run the module as if the operand existed.
    let mir = decode(encoded);
    let native = emit_host_native_object(&mir)
        .expect_err("the direct backend must not lower a call through an unknown operand place");
    assert!(
        native.contains("does not know local `missing_operand`"),
        "{native}"
    );
    let interpreted = run_mir(&mir)
        .expect_err("the interpreter must not execute a call through an unknown operand place");
    assert!(
        interpreted
            .message
            .contains("unknown MIR place `missing_operand`"),
        "{interpreted}"
    );
}

// ---------------------------------------------------------------------------
// Declared types, classes, and places
// ---------------------------------------------------------------------------

const COUNTER: &str = "class Counter:\n    value: int64\n    def bump(mut self):\n        self.value += 1\n    def get(self) -> int64:\n        return self.value\ndef main():\n    mut counter = Counter(value=1)\n    counter.bump()\n    print(counter.get())\n";

#[test]
fn native_backend_rejects_unsupported_declared_types() {
    let mut encoded = encode(COUNTER);
    encoded["classes"][0]["fields"][0]["ty"] =
        returned_view_type(json!({ "Named": ["int64", []] }));
    assert_native_rejects(encoded, "field `Counter.value`");

    let mut encoded = encode(COUNTER);
    let get = function_mut(&mut encoded, "Counter.get");
    get["return_type"] = returned_view_type(json!({ "Named": ["int64", []] }));
    assert_native_rejects(encoded, "return type of `Counter.get`");

    let mut encoded = encode(COUNTER);
    let get = function_mut(&mut encoded, "Counter.get");
    *local_type_mut(get, "%t0") = returned_view_type(json!({ "Named": ["int64", []] }));
    assert_native_rejects(encoded, "local `%t0` on `Counter.get`");

    let mut encoded = encode(INDIRECT);
    let apply = function_mut(&mut encoded, "apply");
    apply["params"][1]["ty"] = returned_view_type(json!({ "Named": ["int64", []] }));
    assert_native_rejects(encoded, "parameter `left` on `apply`");
}

#[test]
fn native_backend_rejects_unknown_classes_and_fields() {
    let mut encoded = encode(COUNTER);
    let main = function_mut(&mut encoded, "main");
    for instruction in instructions_mut(main) {
        if instruction["Assign"]["value"]["Construct"]["class_name"] == json!("Counter") {
            instruction["Assign"]["value"]["Construct"]["class_name"] = json!("Missing");
        }
    }
    assert_native_rejects(encoded, "does not know class `Missing`");

    let mut encoded = encode(COUNTER);
    let get = function_mut(&mut encoded, "Counter.get");
    *assign_mut(get, "%t0") = json!({ "Use": { "Place": "self.bogus" } });
    assert_native_rejects(encoded, "bogus");
}

const OWNED_COUNTER: &str = "class Counter:\n    value: int64\ndef take(counter: own Counter) -> int64:\n    return counter.value\ndef main():\n    print(take(Counter(value=1)))\n";

#[test]
fn native_backend_moves_plain_class_fields_out_of_flattened_roots() {
    let mut encoded = encode(OWNED_COUNTER);
    let take = function_mut(&mut encoded, "take");
    let mut forged = false;
    for instruction in instructions_mut(take) {
        if instruction["Assign"]["value"]["Use"]["Place"] == json!("counter.value") {
            instruction["Assign"]["value"]["Use"] = json!({ "MovePlace": "counter.value" });
            forged = true;
        }
    }
    assert!(forged, "the field read should lower to a place use");
    assert_native_emits(encoded);
}

// ---------------------------------------------------------------------------
// Terminators and operators
// ---------------------------------------------------------------------------

const ASSERTION: &str =
    "def main():\n    value = 1\n    assert value == 1, \"must hold\"\n    print(value)\n";

#[test]
fn native_backend_rejects_malformed_terminators_and_unary_operands() {
    let mut encoded = encode(ASSERTION);
    let main = function_mut(&mut encoded, "main");
    let terminator = main["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .map(|block| &mut block["terminator"])
        .find(|terminator| terminator["AssertFail"].is_object())
        .expect("the assertion should lower to an AssertFail terminator");
    terminator["AssertFail"]["captures"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_native_rejects(encoded, "exactly two assertion captures");

    let mut encoded = encode(ASSERTION);
    let main = function_mut(&mut encoded, "main");
    let mut forged = false;
    for block in main["blocks"].as_array_mut().unwrap() {
        if block["terminator"]["Branch"].is_object() {
            block["terminator"]["Branch"]["condition"] = json!({ "Float": 1.0 });
            forged = true;
        }
    }
    assert!(forged);
    assert_native_rejects(encoded, "cannot use `float64` as a branch condition");

    let mut encoded = encode(ASSERTION);
    let main = function_mut(&mut encoded, "main");
    let mut forged = false;
    for instruction in instructions_mut(main) {
        if instruction["Assign"]["value"]["Binary"]["op"] == json!("Eq") {
            instruction["Assign"]["value"] = json!({
                "Unary": { "op": "Neg", "value": { "Bool": true }, "span": { "line": 3, "column": 12 } }
            });
            forged = true;
        }
    }
    assert!(forged);
    assert_native_rejects(encoded, "does not support unary operation `Neg`");
}

// ---------------------------------------------------------------------------
// select
// ---------------------------------------------------------------------------

const SELECT: &str = "def main():\n    first = Queue[str]()\n    second = Queue[str]()\n    first.put(\"one\")\n    outcome = select(first, second)\n    match own outcome:\n        case SelectOutcome.Queue(index, _):\n            print(index)\n        case _:\n            print(\"other\")\n";

fn select_source_temporaries(function: &mut Value) -> Vec<String> {
    named_call_mut(function, "select")["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argument| argument["value"]["Place"].as_str().unwrap().to_string())
        .collect()
}

fn select_with_second_source_type(ty: Value) -> Value {
    let mut encoded = encode(SELECT);
    let main = function_mut(&mut encoded, "main");
    let sources = select_source_temporaries(main);
    *local_type_mut(main, &sources[1]) = ty;
    encoded
}

#[test]
fn native_backend_rejects_malformed_select_sources() {
    assert_source_emits(SELECT);

    let mut encoded = encode(SELECT);
    named_call_mut(function_mut(&mut encoded, "main"), "select")["args"] = json!([]);
    assert_native_rejects(encoded, "expected `select(source, ...)`");

    let mut encoded = encode(SELECT);
    named_call_mut(function_mut(&mut encoded, "main"), "select")["args"][0]["name"] =
        json!("source");
    assert_native_rejects(encoded, "expected `select(source, ...)`");

    let mut encoded = encode(SELECT);
    named_call_mut(function_mut(&mut encoded, "main"), "select")["args"][0]["value"] =
        json!({ "Place": "unknown_source" });
    assert_native_rejects(encoded, "could not infer a `select` source type");

    assert_native_rejects(
        select_with_second_source_type(json!({ "Named": ["Queue", [{ "Named": ["int64", []] }]] })),
        "inconsistent Queue payload types",
    );
    assert_native_rejects(
        select_with_second_source_type(json!({ "Named": ["Queue", []] })),
        "expected `Queue[Q]` as a `select` source",
    );
    assert_native_rejects(
        select_with_second_source_type(json!({ "Named": ["Task", []] })),
        "expected `Task[T]` as a `select` source",
    );
    assert_native_rejects(
        select_with_second_source_type(json!({ "Named": ["int64", []] })),
        "expected a Queue, Task, or Duration `select` source",
    );

    let mut encoded = encode(SELECT);
    let main = function_mut(&mut encoded, "main");
    let sources = select_source_temporaries(main);
    *local_type_mut(main, &sources[0]) = json!({ "Named": ["Task", [{ "Named": ["int64", []] }]] });
    *local_type_mut(main, &sources[1]) = json!({ "Named": ["Task", [{ "Named": ["str", []] }]] });
    assert_native_rejects(encoded, "inconsistent Task result types");

    let mut encoded = encode(SELECT);
    let main = function_mut(&mut encoded, "main");
    let target = instructions_mut(main)
        .find(|instruction| {
            instruction["Assign"]["value"]["Call"]["callee"]["Name"] == json!("select")
        })
        .map(|instruction| {
            instruction["Assign"]["target"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .expect("select result temporary");
    *local_type_mut(main, &target) = json!({ "Named": ["int64", []] });
    // The `select` result type is inferred from the call itself, not from the
    // temporary's declared local type: the forged `int64` annotation is
    // ignored on both boundaries and the program still observes the queue
    // outcome (index 0).
    assert_accepted_on_both_boundaries(encoded, "0\n");
}

// ---------------------------------------------------------------------------
// Collection member calls
// ---------------------------------------------------------------------------

const COLLECTIONS: &str = "import random\ndef main():\n    mut values = [3, 1, 2]\n    values.append(4)\n    values.pop()\n    values.insert(0, 9)\n    values.remove(9)\n    values.reverse()\n    values.extend([5])\n    values.reserve(2)\n    values.swap(0, 1)\n    values.sort()\n    values.clear()\n    mut names: set[str] = {\"a\"}\n    names.add(\"b\")\n    names.remove(\"b\")\n    names.discard(\"c\")\n    names.reserve(2)\n    names.clear()\n    mut table: dict[str, int64] = {\"a\": 1}\n    table.remove(\"a\")\n    table.reserve(2)\n    table.clear()\n    mut rng = random.Rng(seed=1)\n    rng.shuffle(values)\n    value = 3\n    print(value.to_float())\n    print(Duration.seconds(1).to_ms())\n    print(values.index(1))\n    print(values.count(1))\n    print(values.len())\n";

/// Member calls whose receiver place carries a mutation back to the caller's
/// storage; dropping the place would strand the mutation on a temporary.
const MUTATING_RECEIVER_FIELDS: &[&str] = &[
    "append", "pop", "insert", "remove", "reverse", "extend", "reserve", "swap", "clear", "add",
    "discard", "shuffle",
];

/// Member calls that only read through their receiver place.
const READ_ONLY_RECEIVER_FIELDS: &[&str] = &["to_float", "to_ms", "index", "count", "len"];

#[test]
fn member_calls_without_receiver_places_are_refused_or_lowered_by_field_kind() {
    assert_source_emits(COLLECTIONS);
    let mut fields = Vec::new();
    {
        let mut encoded = encode(COLLECTIONS);
        for instruction in instructions_mut(function_mut(&mut encoded, "main")) {
            if member_has_receiver_place(instruction) {
                fields.push(member_field(instruction).unwrap().to_string());
            }
        }
    }
    assert!(
        fields.len() >= 12,
        "expected many receiver-bearing calls: {fields:?}"
    );
    for (index, field) in fields.iter().enumerate() {
        let mut encoded = encode(COLLECTIONS);
        let mut seen = 0;
        for instruction in instructions_mut(function_mut(&mut encoded, "main")) {
            if member_has_receiver_place(instruction) {
                if seen == index {
                    instruction["Assign"]["value"]["Call"]["callee"]["Member"]["receiver_place"] =
                        json!(null);
                }
                seen += 1;
            }
        }
        let field = field.as_str();
        if MUTATING_RECEIVER_FIELDS.contains(&field) {
            // A mutation without a receiver writeback place is a shared
            // validator rejection, reported identically on both boundaries.
            assert_rejected_on_both_boundaries(
                encoded,
                &format!(
                    "invalid MIR member call `{field}` in `main` requires a mutable receiver writeback place"
                ),
            );
        } else if READ_ONLY_RECEIVER_FIELDS.contains(&field) {
            // A read-only member call needs no writeback: the direct backend
            // lowers it from the object operand alone. (The interpreter still
            // resolves the receiver through its place and fails at execution
            // with `collection value was not found`; that divergence is
            // recorded in the coverage report, not pinned here.)
            assert_native_emits(encoded);
        } else {
            panic!("member call `{field}` must be classified as mutating or read-only");
        }
    }
}

#[test]
fn native_backend_rejects_malformed_collection_member_arguments() {
    let extra = json!({ "name": null, "value": { "Int": 1 }, "writeback_place": null });

    let mut encoded = encode(COLLECTIONS);
    member_call_mut(function_mut(&mut encoded, "main"), "to_float")["args"] = json!([extra]);
    assert_native_rejects(encoded, "expected `to_float()` to take no arguments");

    let mut encoded = encode(COLLECTIONS);
    member_call_mut(function_mut(&mut encoded, "main"), "to_ms")["args"] = json!([extra]);
    assert_native_rejects(encoded, "expected `to_ms()` to take no arguments");

    let mut encoded = encode(COLLECTIONS);
    member_call_mut(function_mut(&mut encoded, "main"), "index")["args"] = json!([]);
    assert_native_rejects(encoded, "expected `index()` to receive one value argument");

    let mut encoded = encode(COLLECTIONS);
    member_call_mut(function_mut(&mut encoded, "main"), "count")["args"] = json!([]);
    assert_native_rejects(encoded, "expected `count()` to receive one value argument");

    let mut encoded = encode(COLLECTIONS);
    member_call_mut(function_mut(&mut encoded, "main"), "shuffle")["args"][0]["writeback_place"] =
        json!(null);
    assert_native_rejects(
        encoded,
        "expected `shuffle()` to carry a mutable argument writeback place",
    );

    let mut encoded = encode(COLLECTIONS);
    let main = function_mut(&mut encoded, "main");
    let mut seen = 0;
    for instruction in instructions_mut(main) {
        if member_field(instruction) == Some("clear") {
            seen += 1;
            if seen == 2 {
                instruction["Assign"]["value"]["Call"]["args"] = json!([extra]);
            }
        }
    }
    assert_eq!(seen, 3, "list, set, and dict clears");
    assert_native_rejects(encoded, "expected `clear()` to take no arguments");

    let mut encoded = encode(COLLECTIONS);
    let call = member_call_mut(function_mut(&mut encoded, "main"), "pop");
    call["callee"]["Member"]["field"] = json!("__take_index_option");
    call["args"] = json!([]);
    assert_native_rejects(
        encoded,
        "internal consuming vector indexing to receive one argument",
    );

    let mut encoded = encode(COLLECTIONS);
    let call = member_call_mut(function_mut(&mut encoded, "main"), "pop");
    call["callee"]["Member"]["field"] = json!("__take_index_option");
    call["callee"]["Member"]["receiver_place"] = json!(null);
    call["args"] = json!([extra]);
    assert_native_rejects(
        encoded,
        "internal consuming vector indexing to provide its private receiver place",
    );

    let mut encoded = encode(COLLECTIONS);
    let call = member_call_mut(function_mut(&mut encoded, "main"), "discard");
    call["callee"]["Member"]["field"] = json!("__take_index_option");
    call["args"] = json!([]);
    assert_native_rejects(
        encoded,
        "internal consuming set indexing to receive one argument",
    );

    let mut encoded = encode(COLLECTIONS);
    let call = member_call_mut(function_mut(&mut encoded, "main"), "discard");
    call["callee"]["Member"]["field"] = json!("__take_index_option");
    call["callee"]["Member"]["receiver_place"] = json!(null);
    call["args"] = json!([extra]);
    assert_native_rejects(
        encoded,
        "internal consuming set indexing to provide its private receiver place",
    );
}

// ---------------------------------------------------------------------------
// Trait dispatch on unions and type parameters
// ---------------------------------------------------------------------------

const UNION_DISPATCH: &str = "trait Speak:\n    def speak(self) -> str\nclass Dog:\n    name: str\nclass Cat:\n    name: str\nimpl Speak for Dog:\n    def speak(self) -> str:\n        return \"woof\"\nimpl Speak for Cat:\n    def speak(self) -> str:\n        return \"meow\"\ndef talk(pet: Dog | Cat) -> str:\n    return pet.speak()\ndef main():\n    print(talk(Dog(name=\"d\")))\n    print(talk(Cat(name=\"c\")))\n";

const GENERIC_TRAIT: &str = "trait Speak:\n    def speak(self) -> str\nclass Dog:\n    name: str\nimpl Speak for Dog:\n    def speak(self) -> str:\n        return \"woof\"\ndef show[T: Speak](value: T) -> str:\n    return value.speak()\ndef main():\n    print(show(Dog(name=\"d\")))\n";

fn trait_member_call_mut(function: &mut Value) -> &mut Value {
    instructions_mut(function)
        .find(|instruction| {
            instruction["Assign"]["value"]["Call"]["callee"]["TraitMember"].is_object()
        })
        .map(|instruction| &mut instruction["Assign"]["value"]["Call"])
        .expect("trait member call")
}

#[test]
fn native_backend_dispatches_union_receivers_across_trait_candidates() {
    assert_source_emits(UNION_DISPATCH);
    assert_source_emits(GENERIC_TRAIT);

    let mut encoded = encode(UNION_DISPATCH);
    let call = trait_member_call_mut(function_mut(&mut encoded, "talk"));
    let member = call["callee"]["TraitMember"].clone();
    call["callee"] = json!({ "Member": {
        "object": member["object"],
        "field": member["field"],
        "receiver_place": member["receiver_place"],
    } });
    // Rewriting the trait callee to a plain member call is not a shared
    // validator rejection. The direct backend resolves `speak` on the union
    // receiver through the same dynamic candidate search it uses for
    // `TraitMember`, so it emits; the interpreter only refuses the call when
    // it executes (a backend divergence recorded in the coverage report).
    let mir = decode(encoded);
    let object = emit_host_native_object(&mir)
        .expect("the direct backend dispatches the rewritten member call dynamically");
    assert!(!object.is_empty());
    let interpreted = run_mir(&mir)
        .expect_err("the interpreter has no dynamic dispatch for a plain member call on a union");
    assert!(
        interpreted
            .message
            .contains("unsupported MIR member call `speak`"),
        "{interpreted}"
    );

    let mut encoded = encode(UNION_DISPATCH);
    let duplicate = encoded["trait_impls"][0].clone();
    encoded["trait_impls"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_native_rejects(encoded, "does not know method");

    let mut encoded = encode(GENERIC_TRAIT);
    trait_member_call_mut(function_mut(&mut encoded, "show"))["callee"]["TraitMember"]["field"] =
        json!("vanish");
    assert_native_rejects(encoded, "does not know dynamic method `.vanish`");

    let mut encoded = encode(GENERIC_TRAIT);
    encoded["trait_impls"][0]["for_type"] = json!({ "Tuple": [{ "Named": ["int64", []] }] });
    assert_native_rejects(encoded, "does not know how to call dynamic method `.speak`");
}

// ---------------------------------------------------------------------------
// Temporaries whose types the backend must infer
// ---------------------------------------------------------------------------

const TEMPORARIES: &str = "enum Mode:\n    Fast\n    Slow\ndef main():\n    mode = Mode.Fast\n    flag: int64 | None = 3\n    factor = 2\n    scale: def(int64) -> int64 = lambda value: value * factor\n    plain: def(int64) -> int64 = lambda value: value + 1\n    print(scale(2))\n    print(plain(2))\n    pair = (1, \"a\")\n    print(pair[0])\n    wait = Duration.seconds(1) + Duration.seconds(2)\n    print(wait)\n    match flag:\n        case int64 as number:\n            print(number)\n        case None:\n            print(0)\n";

/// Drops the declared types of temporaries assigned from the listed rvalue
/// kinds so the backend must infer them from the rvalue itself.
fn strip_temporary_local_types(function: &mut Value, kinds: &[&str]) {
    let stripped: Vec<String> = instructions_mut(function)
        .filter(|instruction| {
            let target = instruction["Assign"]["target"].as_str().unwrap_or("");
            let value = &instruction["Assign"]["value"];
            target.starts_with("%t")
                && kinds.iter().any(|kind| {
                    value[kind].is_object()
                        || (*kind == "MemberCall" && value["Call"]["callee"]["Member"].is_object())
                        || (*kind == "NamedCall" && value["Call"]["callee"]["Name"].is_string())
                        || (*kind == "ValueCall" && value["Call"]["callee"]["Value"].is_object())
                })
        })
        .map(|instruction| {
            instruction["Assign"]["target"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(!stripped.is_empty(), "no temporaries matched {kinds:?}");
    function["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|local| !stripped.contains(&local["name"].as_str().unwrap().to_string()));
}

#[test]
fn native_backend_infers_temporary_types_from_rvalues() {
    assert_source_emits(TEMPORARIES);
    let mut encoded = encode(TEMPORARIES);
    strip_temporary_local_types(
        function_mut(&mut encoded, "main"),
        &[
            "Closure",
            "TupleLiteral",
            "TupleElement",
            "Binary",
            "NamedCall",
            "ValueCall",
        ],
    );
    assert_native_emits(encoded);

    let mut encoded = encode(ARRAYS);
    strip_temporary_local_types(
        function_mut(&mut encoded, "main"),
        &["Binary", "MemberCall"],
    );
    assert_native_emits(encoded);

    // Inference is intentional here as well: the `select` result temporary
    // is typed from the call, so stripping its declared type changes nothing
    // on either boundary.
    let mut encoded = encode(SELECT);
    strip_temporary_local_types(function_mut(&mut encoded, "main"), &["NamedCall"]);
    assert_accepted_on_both_boundaries(encoded, "0\n");
}

// ---------------------------------------------------------------------------
// Resources, tasks, host builtins, try/From
// ---------------------------------------------------------------------------

const RESOURCE: &str = "class Handle:\n    name: str\n    items: list[int64]\n    def close(mut self):\n        print(\"closed\")\ndef push(items: mut list[int64]):\n    items.append(1)\ndef main():\n    with handle = Handle(name=\"h\", items=[]):\n        push(handle.items)\n        print(handle.items.len())\n";

const TASKS: &str = "def work(value: int64) -> int64:\n    return value * 2\ndef main():\n    with group = TaskGroup():\n        task = group.start(work, 1)\n        print(wait_all([task], timeout=Duration.seconds(1)))\n";

const HOST: &str = "import sys\ndef main():\n    print(sys.monotonic_time_ms() >= 0)\n    print(sys.env(\"AURA_COVERAGE_MISSING\"))\n";

const TRY_FROM: &str = "trait From[T]:\n    def from(value: own T) -> Self\nenum ReadError:\n    Missing\nenum AppError:\n    Read(ReadError)\nimpl From[ReadError] for AppError:\n    def from(value: own ReadError) -> AppError:\n        return AppError.Read(value)\ndef read(flag: bool) -> Result[int32, ReadError]:\n    if flag:\n        return Result.Ok(7)\n    return Result.Err(ReadError.Missing)\ndef load(flag: bool) -> Result[int32, AppError]:\n    value = try read(flag)\n    return Result.Ok(value + 1)\ndef main() -> int32:\n    print(load(true))\n    return 0\n";

#[test]
fn native_backend_writes_mutable_projections_through_resource_sinks() {
    assert_source_emits(RESOURCE);
}

#[test]
fn native_backend_rejects_malformed_task_starts_and_waits() {
    assert_source_emits(TASKS);

    let mut encoded = encode(TASKS);
    let call = named_call_mut(function_mut(&mut encoded, "main"), "wait_all");
    call["args"].as_array_mut().unwrap().remove(0);
    assert_native_rejects(encoded, "expected `wait_all(tasks, timeout=...)`");

    let mut encoded = encode(TASKS);
    let main = function_mut(&mut encoded, "main");
    let mut forged = false;
    for instruction in instructions_mut(main) {
        if instruction["Assign"]["value"]["StartTask"].is_object() {
            instruction["Assign"]["value"]["StartTask"]["function"] = json!({ "Place": "%t2" });
            forged = true;
        }
    }
    assert!(forged, "the task start should lower to a StartTask rvalue");
    // A task function operand without a recorded callable identity is a
    // shared validator rejection on both boundaries.
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR task call in `main` has no authoritative callable contract",
    );
}

#[test]
fn native_backend_lowers_host_builtin_calls() {
    assert_source_emits(HOST);
}

#[test]
fn native_backend_rejects_try_conversions_without_from_implementations() {
    assert_source_emits(TRY_FROM);

    let mut encoded = encode(TRY_FROM);
    encoded["trait_impls"] = json!([]);
    assert_native_rejects(
        encoded,
        "could not find `From[ReadError] for AppError` required by `try`",
    );

    let mut encoded = encode(TRY_FROM);
    encoded["trait_impls"][0]["methods"][0]["function_name"] = json!("From.vanished");
    assert_native_rejects(encoded, "does not know From function `From.vanished`");
}

const UNION_MATCH: &str = "def describe(value: int64 | None) -> int64:\n    match value:\n        case int64 as number:\n            return number\n        case None:\n            return 0\ndef main():\n    print(describe(3))\n    print(describe(None))\n";

#[test]
fn unresolvable_goto_labels_are_rejected_by_the_shared_validator() {
    let mut encoded = encode(UNION_MATCH);
    let describe = function_mut(&mut encoded, "describe");
    let mut forged = false;
    for block in describe["blocks"].as_array_mut().unwrap() {
        if block["terminator"]["Goto"].is_string() {
            block["terminator"]["Goto"] = json!("nowhere");
            forged = true;
        }
    }
    assert!(forged, "the union match should lower at least one Goto");
    // The validator's control-flow walk refuses the dangling label before
    // either backend sees the function, so both report the same reason.
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR function `describe` branches to unknown block `nowhere`",
    );
}
