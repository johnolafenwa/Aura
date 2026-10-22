//! The ownership pass and checker (Batch 7 step 8e, part 2). Lowering
//! states every retain and release, both public boundaries check what a
//! module states, and MIR that states nothing gets the pass there.

use aura_compiler::mir::ownership::strip_ownership;
use aura_compiler::mir::MirOwnership;
use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

const PROGRAM: &str = r#"def worker(text: own str) -> int32:
    print(text)
    return 1

def pick(flag: bool) -> str:
    a = f"a{flag}"
    b = f"b{flag}"
    if flag:
        print(a)
    else:
        print(b)
    return "done"

def main():
    name = f"n{1}"
    with TaskGroup() as group:
        task = group.start(worker, name)
        print(task.result())
    print(pick(true))
"#;

const STDOUT: &str = "n1\nTaskResult.Ready(1)\natrue\ndone\n";

fn lowered() -> MirModule {
    lower_source_to_mir(PROGRAM).expect("the program should lower")
}

fn encode(module: &MirModule) -> Value {
    serde_json::to_value(module).expect("MIR should serialize")
}

fn decode(encoded: Value) -> MirModule {
    serde_json::from_value(encoded).expect("forged MIR should deserialize")
}

/// The instructions of the block in `function` holding one `found`
/// accepts, and that instruction's index.
fn around<'a>(
    encoded: &'a mut Value,
    function: &str,
    found: impl Fn(&Value) -> bool,
) -> (&'a mut Vec<Value>, usize) {
    let block = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|candidate| candidate["name"] == function)
        .unwrap()["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["instructions"].as_array().unwrap().iter().any(&found))
        .unwrap_or_else(|| panic!("`{function}` should hold the instruction"));
    let instructions = block["instructions"].as_array_mut().unwrap();
    let index = instructions.iter().position(found).unwrap();
    (instructions, index)
}

fn assert_rejected_on_both_boundaries(encoded: Value, expected: &str) {
    let mir = decode(encoded);
    let interpreted = run_mir(&mir).expect_err("the interpreter must reject the forged module");
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert_eq!(
        interpreted.message.strip_prefix("invalid MIR ownership: "),
        Some(native.as_str()),
        "both boundaries must report the same checker reason"
    );
    assert!(
        native.contains(expected),
        "rejection `{native}` should mention `{expected}`"
    );
}

#[test]
fn lowered_and_stripped_modules_run_and_emit_the_same() {
    let explicit = lowered();
    assert_eq!(explicit.ownership, MirOwnership::Explicit);
    let mut implicit = explicit.clone();
    strip_ownership(&mut implicit);
    assert_eq!(implicit.ownership, MirOwnership::Implicit);
    for module in [&explicit, &implicit] {
        assert_eq!(
            run_mir(module).expect("the module should run").stdout,
            STDOUT
        );
    }
    let object = emit_host_native_object(&explicit).expect("the module should emit");
    assert!(object == emit_host_native_object(&implicit).expect("the module should emit"));
}

#[test]
fn a_double_release_is_refused_on_both_boundaries() {
    let mut encoded = encode(&lowered());
    let release = json!({ "Release": { "place": "a" } });
    let (instructions, index) = around(&mut encoded, "pick", |instruction| *instruction == release);
    instructions.insert(index, release.clone());
    assert_rejected_on_both_boundaries(encoded, "invalid MIR in `pick` releases `a` twice");
}

#[test]
fn a_retained_task_argument_is_refused_on_both_boundaries() {
    let mut encoded = encode(&lowered());
    let (instructions, index) = around(&mut encoded, "main", |instruction| {
        instruction["Assign"]["value"].get("StartTask").is_some()
    });
    instructions[index]["Assign"]["value"]["StartTask"]["args"][0]["value"] =
        json!({ "Place": "name" });
    instructions.insert(index, json!({ "Retain": { "place": "name" } }));
    assert_rejected_on_both_boundaries(encoded, "retains task argument `name`");
}

#[test]
fn a_missing_release_is_refused_on_both_boundaries() {
    let mut encoded = encode(&lowered());
    let release = json!({ "Release": { "place": "group" } });
    let (instructions, index) = around(&mut encoded, "main", |instruction| *instruction == release);
    instructions.remove(index);
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR in `main` returns while `group` still holds a value",
    );
}
