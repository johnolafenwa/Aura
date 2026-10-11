//! Forged-MIR regressions for typed context-manager exit actions. Each case
//! lowers a valid manager program, corrupts one cleanup fact in the
//! serialized module, and asserts that both public boundaries (the
//! interpreter and the direct backend) reject it for the same validator
//! reason.

use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

const MANAGERS: &str = r#"class Guard:
    items: list[int64]

    def __enter__(mut self) -> view list[int64] from self:
        return view self.items

    def __exit__(mut self) -> None:
        print("exit")

class Flush:
    count: int64

    def __exit__(mut self) -> Result[None, str]:
        return Result.Err("bad")

def work() -> Result[int64, str]:
    with sink = Flush(count=1):
        pass
    return Result.Ok(1)

def main():
    with Guard(items=[1, 2]) as items:
        print(items[0])
    match work():
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
"#;

fn encode() -> Value {
    serde_json::to_value(lower_source_to_mir(MANAGERS).expect("source should lower")).unwrap()
}

fn instructions_mut<'a>(encoded: &'a mut Value, function: &str) -> &'a mut Vec<Value> {
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|candidate| candidate["name"] == function)
        .unwrap_or_else(|| panic!("function `{function}` should exist"));
    function["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .map(|block| block["instructions"].as_array_mut().unwrap())
        .find(|instructions| {
            instructions
                .iter()
                .any(|instruction| instruction.get("PushCleanup").is_some())
        })
        .expect("a block should register a cleanup")
}

fn position(instructions: &[Value], matches: impl Fn(&Value) -> bool) -> usize {
    instructions
        .iter()
        .position(matches)
        .expect("expected instruction not found")
}

fn push_cleanup(instructions: &mut [Value]) -> &mut Value {
    let index = position(instructions, |instruction| {
        instruction.get("PushCleanup").is_some()
    });
    &mut instructions[index]["PushCleanup"]
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
fn lowered_manager_module_is_accepted_by_both_boundaries() {
    let mir: MirModule = serde_json::from_value(encode()).unwrap();
    run_mir(&mir).expect("the unmodified module should run");
    emit_host_native_object(&mir).expect("the unmodified module should compile");
}

#[test]
fn manager_exit_registered_before_enter_returns_is_rejected() {
    let mut encoded = encode();
    let instructions = instructions_mut(&mut encoded, "main");
    let enter = position(instructions, |instruction| {
        instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == "__enter__"
    });
    instructions.remove(enter);
    assert_rejected(encoded, "is registered before `Guard.__enter__` returns");
}

#[test]
fn manager_exit_with_wrong_unit_result_kind_is_rejected() {
    let mut encoded = encode();
    let instructions = instructions_mut(&mut encoded, "main");
    push_cleanup(instructions)["action"] = json!({"ManagerExit": {"exit_result": "Result"}});
    assert_rejected(
        encoded,
        "declares a Result exit result that `Guard.__exit__` does not return",
    );
}

#[test]
fn manager_exit_with_wrong_result_kind_is_rejected() {
    let mut encoded = encode();
    let instructions = instructions_mut(&mut encoded, "work");
    push_cleanup(instructions)["action"] = json!({"ManagerExit": {"exit_result": "Unit"}});
    assert_rejected(
        encoded,
        "declares a Unit exit result that `Flush.__exit__` does not return",
    );
}

#[test]
fn manager_registered_as_close_resource_is_rejected() {
    let mut encoded = encode();
    let instructions = instructions_mut(&mut encoded, "work");
    push_cleanup(instructions)["action"] = json!("ResourceClose");
    assert_rejected(encoded, "registers a manager as a `close` resource");
}

#[test]
fn manager_exit_on_non_manager_place_is_rejected() {
    let mut encoded = encode();
    let instructions = instructions_mut(&mut encoded, "work");
    let exit_result = position(instructions, |instruction| {
        instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == "__exit__"
    });
    let result_place = instructions[exit_result]["Assign"]["target"].clone();
    push_cleanup(instructions)["place"] = result_place;
    assert_rejected(encoded, "does not name a class with `__exit__`");
}

#[test]
fn entry_view_outliving_manager_exit_is_rejected() {
    let mut encoded = encode();
    let instructions = instructions_mut(&mut encoded, "main");
    let end_loan = position(instructions, |instruction| {
        instruction["EndLoan"]["loan"] == "items"
    });
    let ended = instructions.remove(end_loan);
    let pop = position(instructions, |instruction| {
        instruction.get("PopCleanup").is_some()
    });
    instructions.insert(pop + 1, ended);
    assert_rejected(encoded, "mutates locked place `%t0`");
}
