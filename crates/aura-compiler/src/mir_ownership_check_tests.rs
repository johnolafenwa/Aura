use super::{validate_ownership, validate_ownership_within};
use crate::mir::ownership::MAX_OWNERSHIP_STATE_CELLS;
use crate::mir::MirModule;
use serde_json::{json, Value};

const PROGRAM: &str = r#"class Named:
    label: str

def worker(text: own str) -> int32:
    print(text)
    return 1

def parse(text: str) -> Result[int64, str]:
    if text == "4":
        return Result.Ok(4)
    return Result.Err(f"bad {text}")

def double(text: str) -> Result[int64, str]:
    note = f"note {text}"
    other = f"other {text}"
    value = try parse(text)
    print(note)
    print(other)
    return Result.Ok(value * 2)

def pick(flag: bool) -> str:
    a = f"a{flag}"
    b = f"b{flag}"
    if flag:
        print(a)
    else:
        print(b)
    return "done"

def swap(flag: bool) -> str:
    mut s = f"s{flag}"
    s = s + "x"
    return s

def push(items: mut list[int64]):
    items.append(1)

def repeat(n: int64, flag: bool) -> int64:
    s = f"s{n}"
    steps = range(n)
    for i in steps:
        if flag:
            print(s)
    return n

def main() -> int32:
    named = Named(label="ada")
    print(named.label)
    name = f"n{1}"
    with TaskGroup() as group:
        task = group.start(worker, name)
        print(task.result())
    mut items = [1]
    push(items)
    names = ["a", "b"]
    view first = names[0]
    print(first)
    print(pick(true))
    print(swap(false))
    print(repeat(2, true))
    # A function value keeps `parse` owning its parameter.
    checker = parse
    print(checker("1"))
    match double("4"):
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
    return 0
"#;

fn lowered() -> Value {
    let module = crate::lower_source_to_mir(PROGRAM).expect("the program should lower");
    serde_json::to_value(module).expect("MIR should serialize")
}

fn decode(encoded: Value) -> MirModule {
    serde_json::from_value(encoded).expect("forged MIR should deserialize")
}

fn checked(encoded: Value) -> Result<(), String> {
    validate_ownership(&decode(encoded))
}

#[track_caller]
fn assert_rejected(encoded: Value, expected: &str) {
    let error = checked(encoded).expect_err("the checker must reject the forged module");
    assert!(
        error.contains(expected),
        "rejection `{error}` should mention `{expected}`"
    );
}

fn function_mut<'a>(encoded: &'a mut Value, name: &str) -> &'a mut Value {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn blocks_mut<'a>(encoded: &'a mut Value, function: &str) -> &'a mut Vec<Value> {
    function_mut(encoded, function)["blocks"]
        .as_array_mut()
        .unwrap()
}

/// The instructions of the block in `function` that holds an instruction
/// `found` accepts, and that instruction's index.
fn around<'a>(
    encoded: &'a mut Value,
    function: &str,
    found: impl Fn(&Value) -> bool,
) -> (&'a mut Vec<Value>, usize) {
    let block = blocks_mut(encoded, function)
        .iter_mut()
        .find(|block| block["instructions"].as_array().unwrap().iter().any(&found))
        .unwrap_or_else(|| panic!("`{function}` should hold the instruction"));
    let instructions = block["instructions"].as_array_mut().unwrap();
    let index = instructions.iter().position(found).unwrap();
    (instructions, index)
}

fn is(expected: Value) -> impl Fn(&Value) -> bool {
    move |instruction| *instruction == expected
}

fn release(place: &str) -> Value {
    json!({ "Release": { "place": place } })
}

fn retain(place: &str) -> Value {
    json!({ "Retain": { "place": place } })
}

fn assigns(target: &'static str) -> impl Fn(&Value) -> bool {
    move |instruction| instruction["Assign"]["target"] == target
}

fn try_value(encoded: &mut Value) -> &mut Value {
    let (instructions, index) = around(encoded, "double", |instruction| {
        instruction["Assign"]["value"].get("Try").is_some()
    });
    &mut instructions[index]["Assign"]["value"]["Try"]
}

/// `encoded` with the instruction `found` accepts in `function` replaced
/// by `replacement`, which may hold several instructions or none.
fn replaced(
    mut encoded: Value,
    function: &str,
    found: impl Fn(&Value) -> bool,
    replacement: Vec<Value>,
) -> Value {
    let (instructions, index) = around(&mut encoded, function, found);
    instructions.splice(index..=index, replacement);
    encoded
}

/// `encoded` with `inserted` placed right before (`offset` 0) or after
/// (`offset` 1) the instruction `found` accepts.
fn inserted(
    mut encoded: Value,
    function: &str,
    found: impl Fn(&Value) -> bool,
    offset: usize,
    inserted: Value,
) -> Value {
    let (instructions, index) = around(&mut encoded, function, found);
    instructions.insert(index + offset, inserted);
    encoded
}

#[test]
fn the_pass_output_checks_and_an_implicit_module_states_nothing() {
    let encoded = lowered();
    assert_eq!(encoded["ownership"], "Explicit");
    checked(encoded.clone()).expect("the pass output should check");
    // An implicit module passes whatever it holds.
    let mut forged = inserted(encoded, "pick", assigns("a"), 0, release("a"));
    forged["ownership"] = json!("Implicit");
    checked(forged).expect("an implicit module states no ownership");
}

#[test]
fn a_double_release_is_refused() {
    assert_rejected(
        inserted(lowered(), "pick", is(release("a")), 0, release("a")),
        "releases `a` twice",
    );
}

#[test]
fn a_use_after_a_release_is_refused() {
    // In `pick`, release `a` before the print reads it.
    let encoded = replaced(lowered(), "pick", is(release("a")), Vec::new());
    let encoded = inserted(
        encoded,
        "pick",
        |instruction| instruction["Assign"]["target"] == "%t4",
        0,
        release("a"),
    );
    assert_rejected(encoded, "uses `a` after releasing it");
    // A store into a released aggregate is refused as well.
    let encoded = inserted(
        lowered(),
        "main",
        is(release("named")),
        1,
        json!({ "Assign": { "target": "named.label", "value": { "Use": { "String": "x" } } } }),
    );
    assert_rejected(encoded, "stores into `named.label` after releasing it");
}

#[test]
fn a_use_after_a_move_is_refused() {
    let encoded = lowered();
    let mut forged = encoded.clone();
    let (instructions, index) = around(&mut forged, "main", assigns("named"));
    let moved = instructions[index].clone();
    instructions.insert(index + 1, moved);
    assert_rejected(forged, "moves `%t0` after moving it");
    assert_rejected(
        inserted(encoded, "main", assigns("named"), 1, release("%t0")),
        "releases `%t0` after moving it",
    );
}

#[test]
fn values_used_or_released_before_they_hold_one_are_refused() {
    let encoded = lowered();
    assert_rejected(
        inserted(encoded.clone(), "pick", assigns("b"), 0, release("b")),
        "releases `b` before it holds a value",
    );
    assert_rejected(
        inserted(encoded, "pick", assigns("b"), 0, retain("b")),
        "retains `b` before it holds a value",
    );
}

#[test]
fn a_release_on_only_one_side_of_a_branch_is_refused() {
    let encoded = replaced(lowered(), "pick", is(release("a")), Vec::new());
    assert_rejected(encoded.clone(), "returns while `a` may still hold a value");
    // Storing over the value joined from both sides is refused too.
    let mut forged = encoded;
    let join = blocks_mut(&mut forged, "pick")
        .iter_mut()
        .find(|block| block["terminator"].get("Return").is_some())
        .unwrap();
    join["instructions"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "Assign": { "target": "a", "value": { "Use": { "String": "x" } } } }));
    assert_rejected(
        forged,
        "assigns `a`, which holds a value on only some paths",
    );
}

#[test]
fn a_value_released_on_one_path_through_a_loop_is_refused() {
    // `s` lives across the loop in `repeat`; releasing it on the branch
    // that prints leaves it held on only some paths at the loop header.
    let encoded = inserted(
        lowered(),
        "repeat",
        |instruction| instruction["Assign"]["value"]["Call"]["args"][0]["value"]["Place"] == "s",
        1,
        release("s"),
    );
    assert_rejected(encoded, "uses `s`, which holds a value on only some paths");
}

#[test]
fn a_store_over_a_held_value_is_refused() {
    assert_rejected(
        replaced(lowered(), "swap", is(release("s")), Vec::new()),
        "assigns `s` while it still holds a value",
    );
}

#[test]
fn a_release_under_a_loan_or_a_cleanup_is_refused() {
    let encoded = lowered();
    let begins_loan = |instruction: &Value| instruction.get("BeginElementLoan").is_some();
    assert_rejected(
        inserted(encoded.clone(), "main", begins_loan, 1, release("names")),
        "releases `names` while loan `first` borrows it",
    );
    let push_cleanup = |instruction: &Value| instruction.get("PushCleanup").is_some();
    assert_rejected(
        inserted(encoded.clone(), "main", push_cleanup, 1, release("group")),
        "releases `group` while its cleanup is registered",
    );
    // A move is held to the same rule.
    let moved =
        json!({ "Assign": { "target": "%t99", "value": { "Use": { "MovePlace": "group" } } } });
    let mut forged = inserted(encoded, "main", push_cleanup, 1, moved);
    function_mut(&mut forged, "main")["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t99", "ty": { "Named": ["TaskGroup", []] } }));
    assert_rejected(forged, "moves `group` while its cleanup is registered");
}

#[test]
fn a_wrong_try_list_is_refused() {
    for listed in [json!(["note"]), json!([]), json!(["note", "other", "text"])] {
        let mut encoded = lowered();
        try_value(&mut encoded)["releases_on_error"] = listed;
        assert_rejected(encoded, "but its error edge must release [note, other]");
    }
}

#[test]
fn a_parameter_the_caller_keeps_is_never_released_or_moved() {
    let encoded = lowered();
    let borrowing = encoded["borrowing"].as_array().unwrap();
    assert!(borrowing.contains(&json!("double")), "{borrowing:?}");
    assert!(!borrowing.contains(&json!("parse")), "{borrowing:?}");
    assert_rejected(
        inserted(
            encoded.clone(),
            "double",
            assigns("note"),
            1,
            release("text"),
        ),
        "releases `text`, which its caller keeps",
    );
    let mut forged = encoded;
    let (instructions, index) = around(&mut forged, "double", |instruction| {
        instruction["Assign"]["value"].get("Try").is_some()
    });
    instructions.insert(
        index + 1,
        json!({ "Assign": { "target": "%t99", "value": { "Use": { "MovePlace": "text" } } } }),
    );
    instructions.insert(index + 2, release("%t99"));
    function_mut(&mut forged, "double")["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t99", "ty": { "Named": ["str", []] } }));
    assert_rejected(forged, "returns without `text`, which its caller keeps");
}

#[test]
fn a_recorded_function_reached_another_way_is_refused() {
    let mut encoded = lowered();
    encoded["borrowing"]
        .as_array_mut()
        .unwrap()
        .push(json!("parse"));
    assert_rejected(
        encoded,
        "`parse` keeps its callers' shared arguments, but a call can reach it another way",
    );
}

#[test]
fn a_retained_task_argument_is_refused() {
    let mut encoded = lowered();
    let (instructions, index) = around(&mut encoded, "main", |instruction| {
        instruction["Assign"]["value"].get("StartTask").is_some()
    });
    instructions[index]["Assign"]["value"]["StartTask"]["args"][0]["value"] =
        json!({ "Place": "name" });
    instructions.insert(index, retain("name"));
    assert_rejected(
        encoded,
        "retains task argument `name`, but a task copies its arguments",
    );
}

#[test]
fn retains_match_consuming_operands_one_to_one() {
    let encoded = lowered();
    assert_rejected(
        replaced(encoded.clone(), "double", is(retain("text")), Vec::new()),
        "consumes `text` without a retain",
    );
    // A retain of a borrowed operand is left over.
    assert_rejected(
        inserted(encoded.clone(), "pick", is(release("a")), 0, retain("a")),
        "leaves a retain of `a` that no operand takes",
    );
    // A terminator takes no retain either.
    let mut forged = encoded.clone();
    let returns = blocks_mut(&mut forged, "swap")
        .iter_mut()
        .find(|block| block["terminator"].get("Return").is_some())
        .unwrap();
    returns["instructions"]
        .as_array_mut()
        .unwrap()
        .push(retain("s"));
    assert_rejected(forged, "leaves a retain of `s` that no operand takes");
    assert_rejected(
        inserted(encoded.clone(), "pick", is(release("a")), 0, retain("flag")),
        "retains `flag`, whose value needs no drop",
    );
    assert_rejected(
        inserted(encoded, "pick", is(release("a")), 0, retain("missing")),
        "retains `missing`, whose value needs no drop",
    );
}

#[test]
fn a_mutable_argument_written_back_to_its_root_moves_without_a_retain() {
    let encoded = lowered();
    let calls_push =
        |instruction: &Value| instruction["Assign"]["value"]["Call"]["callee"]["Name"] == "push";
    // `push(items)` takes the value of `items` and its writeback refills
    // it, so a retain is left over and a release leaves nothing to move.
    assert_rejected(
        inserted(encoded.clone(), "main", calls_push, 0, retain("items")),
        "leaves a retain of `items` that no operand takes",
    );
    assert_rejected(
        inserted(encoded, "main", calls_push, 0, release("items")),
        "moves `items` after releasing it",
    );
}

#[test]
fn a_release_names_a_root_whose_value_needs_drop() {
    for place in ["flag", "a.len", "missing"] {
        assert_rejected(
            inserted(lowered(), "pick", is(release("a")), 0, release(place)),
            &format!("releases `{place}`, which is not a root whose value needs drop"),
        );
    }
}

#[test]
fn returns_leave_every_root_empty_but_mut_parameters() {
    let encoded = lowered();
    let release_range = |instruction: &Value| instruction["Release"]["place"] == "steps";
    assert_rejected(
        replaced(encoded.clone(), "repeat", release_range, Vec::new()),
        "still holds a value",
    );
    let mut forged = encoded;
    blocks_mut(&mut forged, "push")[0]["instructions"]
        .as_array_mut()
        .unwrap()
        .push(release("items"));
    assert_rejected(
        forged,
        "returns without the value of `mut` parameter `items`",
    );
}

#[test]
fn unreadable_graphs_and_budgets_are_refused() {
    let encoded = lowered();
    let mut forged = encoded.clone();
    // The loan context reads only the reachable blocks.
    blocks_mut(&mut forged, "pick").push(json!({
        "label": "stray",
        "instructions": [],
        "terminator": { "Goto": "nowhere" }
    }));
    assert_rejected(
        forged,
        "has a block graph the ownership checker cannot read",
    );

    let module = decode(encoded);
    let error = validate_ownership_within(&module, 1, None).unwrap_err();
    assert!(
        error.contains("is too large for the ownership checker"),
        "{error}"
    );
    let error = validate_ownership_within(&module, MAX_OWNERSHIP_STATE_CELLS, Some(1)).unwrap_err();
    assert!(
        error.contains("exceeds the ownership checker's step budget"),
        "{error}"
    );
}
