//! The explicit-ownership vocabulary of MIR (Batch 7 step 8e, part 1):
//! `Retain`, `Release`, `Try` releases, the module ownership flag, and the
//! Copy marks on classes and enums. Both backends treat the instructions as
//! no-ops, and the shared validator refuses malformed uses on both public
//! boundaries.

use aura_compiler::mir::ownership::strip_ownership;
use aura_compiler::mir::{MirOwnership, Rvalue};
use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

const PROGRAM: &str = r#"copy class Point:
    x: int64
    y: int64

class Named:
    label: str

class Holder[T]:
    item: T

enum Shade:
    Light
    Dark(int64)

enum Tagged:
    Plain
    Text(str)

enum Maybe[T]:
    Some(T)
    Empty

def parse(text: str) -> Result[int64, str]:
    if text == "4":
        return Result.Ok(4)
    return Result.Err(f"bad {text}")

def double(text: str) -> Result[int64, str]:
    note = f"note {text}"
    value = try parse(text)
    print(note)
    return Result.Ok(value * 2)

def main():
    named = Named(label="ada")
    point = Point(x=1, y=2)
    holder = Holder[int64](item=3)
    shade = Shade.Dark(5)
    tagged = Tagged.Text("t")
    maybe = Maybe[int64].Some(6)
    names = ["a", "b"]
    for name in names:
        print(name)
    view first = names[0]
    print(first)
    print(named.label)
    print(point.x + point.y + holder.item)
    match double("4"):
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
    match double("x"):
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
"#;

const STDOUT: &str = "a\nb\na\nada\n6\nnote 4\n8\nbad x\n";

fn lowered() -> MirModule {
    lower_source_to_mir(PROGRAM).expect("the program should lower")
}

fn encode(module: &MirModule) -> Value {
    serde_json::to_value(module).expect("MIR should serialize")
}

fn decode(encoded: Value) -> MirModule {
    serde_json::from_value(encoded).expect("forged MIR should deserialize")
}

fn function_mut<'a>(encoded: &'a mut Value, name: &str) -> &'a mut Value {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn entry_instructions(function: &mut Value) -> &mut Vec<Value> {
    function["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
}

fn explicit(mut encoded: Value) -> Value {
    encoded["ownership"] = json!("Explicit");
    encoded
}

/// `main` with `instruction` inserted after its first instruction.
fn with_main_instruction(encoded: Value, instruction: Value) -> Value {
    let mut encoded = explicit(encoded);
    entry_instructions(function_mut(&mut encoded, "main")).insert(1, instruction);
    encoded
}

/// The `try` in `double` with its releases set to `releases`.
fn with_try_releases(encoded: Value, releases: Value) -> Value {
    let mut encoded = explicit(encoded);
    let double = function_mut(&mut encoded, "double");
    let try_value = double["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
        .find_map(|instruction| {
            instruction
                .get_mut("Assign")
                .and_then(|assign| assign["value"].get_mut("Try"))
        })
        .expect("`double` should hold a `try`");
    try_value["releases_on_error"] = releases;
    encoded
}

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

/// The lowered module without what the ownership pass added.
fn implicit() -> MirModule {
    let mut module = lowered();
    strip_ownership(&mut module);
    module
}

#[test]
fn implicit_mir_serializes_without_the_new_fields() {
    let module = implicit();
    assert_eq!(module.ownership, MirOwnership::Implicit);
    assert_eq!(run_mir(&module).expect("the program runs").stdout, STDOUT);
    let encoded = encode(&module);
    assert!(encoded.get("ownership").is_none());
    let text = encoded.to_string();
    assert!(!text.contains("releases_on_error"));
    assert!(!text.contains("\"Retain\"") && !text.contains("\"Release\""));
    // MIR written before the fields existed reads back with the defaults.
    let decoded = decode(encoded);
    assert_eq!(decoded.ownership, MirOwnership::Implicit);
    let double = decoded
        .functions
        .iter()
        .find(|function| function.name == "double")
        .unwrap();
    assert!(double
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(
            instruction,
            aura_compiler::mir::Instruction::Assign {
                value: Rvalue::Try { releases_on_error, .. },
                ..
            } if releases_on_error.is_empty()
        )));
}

#[test]
fn lowering_marks_copy_classes_and_enums() {
    let module = lowered();
    let class_copy = |name: &str| {
        module
            .classes
            .iter()
            .find(|class| class.name == name)
            .unwrap_or_else(|| panic!("class `{name}` should lower"))
            .copy
    };
    let enum_copy = |name: &str| {
        module
            .enums
            .iter()
            .find(|enum_decl| enum_decl.name == name)
            .unwrap_or_else(|| panic!("enum `{name}` should lower"))
            .copy
    };
    assert!(class_copy("Point"));
    assert!(!class_copy("Named"));
    assert!(!class_copy("Holder"), "a generic class needs drop");
    assert!(enum_copy("Shade"));
    assert!(!enum_copy("Tagged"));
    assert!(!enum_copy("Maybe"), "a type-parameter payload needs drop");
    assert!(!enum_copy("Result"));
    // The marks round-trip, and a module without them needs drop.
    let mut encoded = encode(&module);
    let marks = |module: &MirModule| {
        module
            .classes
            .iter()
            .map(|class| class.copy)
            .chain(module.enums.iter().map(|enum_decl| enum_decl.copy))
            .collect::<Vec<_>>()
    };
    assert_eq!(marks(&decode(encoded.clone())), marks(&module));
    for class in encoded["classes"].as_array_mut().unwrap() {
        class.as_object_mut().unwrap().remove("copy");
    }
    assert!(decode(encoded).classes.iter().all(|class| !class.copy));
}

#[test]
fn backends_treat_retain_release_and_try_releases_as_no_ops() {
    // Lowering states ownership; the stripped module states none, so both
    // backends must do the same with either.
    let explicit = lowered();
    assert_eq!(explicit.ownership, MirOwnership::Explicit);
    let text = encode(&explicit).to_string();
    assert!(text.contains("\"Retain\"") && text.contains("\"Release\""));
    assert!(text.contains("\"releases_on_error\":[\"note\"]"));
    assert_eq!(
        run_mir(&explicit).expect("the module should run").stdout,
        STDOUT
    );
    let base = emit_host_native_object(&implicit()).expect("the program should emit");
    let object = emit_host_native_object(&explicit).expect("the module should emit");
    assert!(object == base, "the vocabulary must not change the object");
}

#[test]
fn an_implicit_module_states_no_ownership() {
    let base = encode(&implicit());
    for instruction in [
        json!({ "Retain": { "place": "named" } }),
        json!({ "Release": { "place": "named" } }),
    ] {
        let mut encoded = with_main_instruction(base.clone(), instruction);
        encoded["ownership"] = json!("Implicit");
        assert_rejected_on_both_boundaries(encoded, "whose ownership is implicit");
    }
    let mut encoded = with_try_releases(base, json!(["note"]));
    encoded["ownership"] = json!("Implicit");
    assert_rejected_on_both_boundaries(encoded, "whose ownership is implicit");
}

#[test]
fn a_release_names_a_root_local_no_loan_holds() {
    let base = encode(&lowered());
    for place in ["named.label", "missing", "", "named..label"] {
        assert_rejected_on_both_boundaries(
            with_main_instruction(base.clone(), json!({ "Release": { "place": place } })),
            "which is not a root local",
        );
    }
    // The view `first` holds an element loan of `names`.
    let mut encoded = explicit(base);
    let main = function_mut(&mut encoded, "main");
    let block = main["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| {
            block["instructions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|instruction| instruction.get("BeginElementLoan").is_some())
        })
        .expect("the view begins an element loan");
    let instructions = block["instructions"].as_array_mut().unwrap();
    let begin = instructions
        .iter()
        .position(|instruction| instruction.get("BeginElementLoan").is_some())
        .unwrap();
    instructions.insert(begin + 1, json!({ "Release": { "place": "names" } }));
    assert_rejected_on_both_boundaries(encoded, "moves locked place `names`");
}

#[test]
fn a_retain_names_a_known_place() {
    let base = encode(&lowered());
    assert_rejected_on_both_boundaries(
        with_main_instruction(
            base.clone(),
            json!({ "Retain": { "place": "missing.label" } }),
        ),
        "retains unknown place `missing.label`",
    );
    assert_rejected_on_both_boundaries(
        with_main_instruction(base, json!({ "Retain": { "place": "named..label" } })),
        "empty MIR place segment",
    );
}

#[test]
fn try_releases_name_distinct_root_locals_in_name_order() {
    let base = encode(&lowered());
    assert_rejected_on_both_boundaries(
        with_try_releases(base.clone(), json!(["value", "note"])),
        "out of name order or twice",
    );
    assert_rejected_on_both_boundaries(
        with_try_releases(base.clone(), json!(["note", "note"])),
        "out of name order or twice",
    );
    assert_rejected_on_both_boundaries(
        with_try_releases(base, json!(["note.len"])),
        "which is not a root local",
    );
}

#[test]
fn a_copy_mark_on_a_value_that_needs_drop_is_refused() {
    let base = encode(&lowered());
    for (kind, name, message) in [
        ("classes", "Named", "class `Named` is marked Copy"),
        ("classes", "Holder", "class `Holder` is marked Copy"),
        ("enums", "Tagged", "enum `Tagged` is marked Copy"),
    ] {
        let mut encoded = base.clone();
        let entry = encoded[kind]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["name"] == name)
            .unwrap();
        entry["copy"] = json!(true);
        assert_rejected_on_both_boundaries(encoded, message);
    }
}
