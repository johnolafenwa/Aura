//! Whole-program contracts that reach MIR interpreter and shared runtime
//! branches no existing fixture exercised: shared module-constant member
//! reads, loan writes that materialize module constants, floating powers, and
//! forged member calls on union receivers.

use aura_compiler::{lower_source_to_mir, run_mir, run_source, MirModule};
use serde_json::Value;

fn run_stdout(source: &str) -> String {
    run_source(source)
        .unwrap_or_else(|error| panic!("program should run: {}", error.message))
        .stdout
}

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

fn member_field(instruction: &Value) -> Option<&str> {
    instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"].as_str()
}

#[test]
fn shared_module_constant_instances_read_their_fields_in_place() {
    let source = r#"
class Point:
    x: int64
    y: int64

    def sum(self) -> int64:
        return self.x + self.y

ORIGIN = Point(x=3, y=4)

def main():
    print(ORIGIN.x)
    print(ORIGIN.sum())
"#;
    assert_eq!(run_stdout(source), "3\n7\n");
}

#[test]
fn loan_writes_materialize_shared_module_constants_and_propagate_try_returns() {
    let source = r#"
LIMIT = 5

def parse(text: str) -> Result[int64, str]:
    if text == "bad":
        return Result.Err("unparsable")
    return Result.Ok(7)

def fill(text: str) -> Result[int64, str]:
    mut value = 0
    view mut slot = value
    slot = try parse(text)
    return Result.Ok(value)

def main():
    mut count = 1
    view mut slot = count
    slot = LIMIT
    print(count)
    print(fill("bad"))
    print(fill("ok"))
"#;
    let encoded = serde_json::to_string(&encode(source)).unwrap();
    assert!(
        encoded.contains("WriteLoan"),
        "view assignments should lower to loan writes"
    );
    assert_eq!(
        run_stdout(source),
        "5\nResult.Err(unparsable)\nResult.Ok(7)\n"
    );
}

#[test]
fn floating_powers_and_divmod_evaluate_on_the_interpreter() {
    let source = r#"
def main():
    base = 2.0
    print(base ** 3.0)
    print(divmod(7, 2))
    print(divmod(-7.5, 2.0))
"#;
    assert_eq!(run_stdout(source), "8.0\n(3, 1)\n(-4.0, 0.5)\n");
}

#[test]
fn union_clone_calls_with_forged_arguments_are_rejected_at_runtime() {
    let source = r#"
def main():
    value: int64 | None = 1
    copy = value.clone()
    print(copy)
"#;
    assert_eq!(run_stdout(source), "1\n");
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let clone = find_instruction(main, |instruction| {
        member_field(instruction) == Some("clone")
    });
    clone["Assign"]["value"]["Call"]["args"] = serde_json::json!([
        { "name": null, "value": { "Int": 1 }, "writeback_place": null }
    ]);
    let forged: MirModule = serde_json::from_value(encoded).expect("forged MIR deserializes");
    let error = run_mir(&forged).expect_err("clone must not accept arguments");
    assert!(
        error.message.contains("`clone` does not take arguments"),
        "{}",
        error.message
    );
}

#[test]
fn interpreter_panics_from_forged_array_constructors_become_diagnostics() {
    let source = r#"
def main():
    values = Array[int64].from_list([1, 2], [2])
    print(values.len())
"#;
    assert_eq!(run_stdout(source), "2\n");
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    let constructor = find_instruction(main, |instruction| {
        instruction["Assign"]["value"]["Call"]["callee"]["Name"] == "Array.from_list"
    });
    constructor["Assign"]["value"]["Call"]["args"][0]["value"] = serde_json::json!({ "Int": 1 });
    let forged: MirModule = serde_json::from_value(encoded).expect("forged MIR deserializes");
    let error = run_mir(&forged).expect_err("a non-list source must not construct an Array");
    assert_eq!(
        error.message,
        "Aura MIR runtime panicked while executing the program"
    );
}
