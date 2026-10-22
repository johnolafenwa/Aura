//! Forged `Span[T]` loans must be refused by the shared MIR validator.
//!
//! Each case lowers a checked span program, edits one loan instruction in the
//! serialized MIR, and expects the same refusal from the interpreter and the
//! direct backend before anything runs.

use aura_compiler::mir::ownership::strip_ownership;
use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};

use serde_json::Value;

/// Lowers `source` to MIR that states no ownership. A forgery edits it, and
/// each boundary then fills the ownership in.
fn lower_implicit(source: &str) -> aura_compiler::Result<MirModule> {
    let mut mir = lower_source_to_mir(source)?;
    strip_ownership(&mut mir);
    Ok(mir)
}

const FILL_SOURCE: &str = "def main():\n    mut items = [1, 2, 3, 4]\n    view mut a = items[0:2]\n    a.fill(9)\n    print(items)\n";

fn assert_rejected_on_both_boundaries(encoded: Value) -> String {
    let mir: MirModule =
        serde_json::from_value(encoded).expect("forged span module remains syntactically valid");
    let interpreted = run_mir(&mir).expect_err("the interpreter must reject the forged module");
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert_eq!(
        interpreted.message.strip_prefix("invalid MIR loan flow: "),
        Some(native.as_str()),
        "both boundaries must report the same shared validator reason"
    );
    native
}

fn main_instructions(encoded: &mut Value) -> &mut Vec<Value> {
    let main = encoded["functions"]
        .as_array_mut()
        .expect("functions are an array")
        .iter_mut()
        .find(|function| function["name"] == "main")
        .expect("lowered MIR has main");
    main["blocks"][0]["instructions"]
        .as_array_mut()
        .expect("instructions are an array")
}

fn position(instructions: &[Value], kind: &str, loan: &str) -> usize {
    instructions
        .iter()
        .position(|instruction| instruction[kind]["loan"] == loan)
        .unwrap_or_else(|| panic!("lowered MIR has {kind} for `{loan}`"))
}

fn fill_call(instructions: &[Value]) -> usize {
    instructions
        .iter()
        .position(|instruction| {
            instruction["Assign"]["value"]["Call"]["callee"]["Runtime"]["op"]
                == serde_json::json!({ "Span": "Fill" })
        })
        .expect("lowered MIR fills through the span")
}

fn lowered_fill() -> Value {
    let mir = lower_implicit(FILL_SOURCE).expect("the span program lowers");
    let output = run_mir(&mir).expect("the unforged span program runs");
    assert_eq!(output.stdout, "[9, 9, 3, 4]\n");
    serde_json::to_value(mir).expect("MIR serializes")
}

#[test]
fn a_mutating_span_member_through_a_shared_loan_is_refused() {
    let mut encoded = lowered_fill();
    let instructions = main_instructions(&mut encoded);
    let begin = position(instructions, "BeginLoan", "a");
    instructions[begin]["BeginLoan"]["mutable"] = Value::Bool(false);
    let reason = assert_rejected_on_both_boundaries(encoded);
    assert!(
        reason.contains("mutates through shared loan `a`"),
        "{reason}"
    );
}

#[test]
fn an_overlapping_mutable_range_is_refused() {
    let mut encoded = lowered_fill();
    let instructions = main_instructions(&mut encoded);
    let begin = position(instructions, "BeginLoan", "a");
    let mut forged = instructions[begin].clone();
    forged["BeginLoan"]["loan"] = Value::String("b".to_string());
    instructions.insert(begin + 1, forged);
    let end = position(instructions, "EndLoan", "a");
    instructions.insert(end, serde_json::json!({ "EndLoan": { "loan": "b" } }));
    let reason = assert_rejected_on_both_boundaries(encoded);
    assert!(
        reason.contains("overlaps active mutable loan `a`"),
        "{reason}"
    );
}

#[test]
fn a_range_used_after_its_base_loan_ends_is_refused() {
    let mut encoded = lowered_fill();
    let instructions = main_instructions(&mut encoded);
    let end = position(instructions, "EndLoan", "a");
    let end_loan = instructions.remove(end);
    let fill = fill_call(instructions);
    instructions.insert(fill, end_loan);
    let reason = assert_rejected_on_both_boundaries(encoded);
    assert!(reason.contains("uses ended loan `a`"), "{reason}");
}

fn forged_fill_message(edit: impl FnOnce(&mut Value)) -> String {
    let mut encoded = lowered_fill();
    let instructions = main_instructions(&mut encoded);
    let call = fill_call(instructions);
    edit(&mut instructions[call]["Assign"]["value"]["Call"]);
    let mir: MirModule =
        serde_json::from_value(encoded).expect("forged span module remains syntactically valid");
    run_mir(&mir)
        .expect_err("the interpreter must reject the forged span call")
        .message
}

#[test]
fn forged_span_calls_are_refused_by_the_interpreter() {
    // A span op the fill's arguments bind to: `restore` takes the start
    // and an `int64` where its values belong, and `eq` takes the fill
    // value where its other list belongs.
    let set_op = |op: &str, arity: usize| {
        let op = op.to_string();
        move |call: &mut Value| {
            call["callee"]["Runtime"]["op"] = serde_json::json!({ "Span": op });
            let args = call["args"].as_array_mut().expect("span args are an array");
            while args.len() < arity {
                args.push(args[0].clone());
            }
            args.truncate(arity);
        }
    };
    let cases: Vec<(Box<dyn FnOnce(&mut Value)>, &str)> = vec![
        (
            // A private member no op names stays a member call.
            Box::new(|call: &mut Value| {
                let runtime = call["callee"]["Runtime"].clone();
                call["callee"] = serde_json::json!({ "Member": {
                    "object": runtime["receiver"],
                    "field": "__span_bogus",
                    "receiver_place": runtime["receiver_place"],
                }});
            }),
            "unknown internal span operation `__span_bogus`",
        ),
        (
            Box::new(|call: &mut Value| {
                call["args"][0]["value"] = serde_json::json!({ "Bool": true })
            }),
            "internal span operation `__span_fill` expects an integer operand 0",
        ),
        (
            Box::new(set_op("Restore", 2)),
            "span restore expects a list operand",
        ),
        (
            Box::new(set_op("Eq", 5)),
            "span equality expects a list operand",
        ),
    ];
    for (edit, expected) in cases {
        assert_eq!(forged_fill_message(edit), expected);
    }
}
