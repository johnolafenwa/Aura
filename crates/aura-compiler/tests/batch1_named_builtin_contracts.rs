//! Forged-MIR regressions for the shared validator's named-builtin contract
//! table. `Array.zeros`, `Array.full`, and `Array.from_list` are dispatched
//! by name without a MIR declaration to bind against; every forged arity,
//! argument name, operand type, writeback, or result type must be refused by
//! BOTH public boundaries (`run_mir` and `emit_host_native_object`) with the
//! same validator reason, before either backend relies on the operand shapes
//! the checker guaranteed.

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

/// The whole `Assign` instruction whose rvalue is the named builtin call.
fn named_call_instruction_mut<'a>(function: &'a mut Value, name: &str) -> &'a mut Value {
    for block in function["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction["Assign"]["value"]["Call"]["callee"]["Name"] == json!(name) {
                return instruction;
            }
        }
    }
    panic!("named call `{name}` not found");
}

fn call_args_mut<'a>(function: &'a mut Value, name: &str) -> &'a mut Vec<Value> {
    named_call_instruction_mut(function, name)["Assign"]["value"]["Call"]["args"]
        .as_array_mut()
        .unwrap()
}

fn retype_call_result(function: &mut Value, name: &str, ty: Value) {
    let target = named_call_instruction_mut(function, name)["Assign"]["target"]
        .as_str()
        .expect("the builtin call assigns a place")
        .to_owned();
    let local = function["local_types"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|local| local["name"] == json!(target))
        .expect("the call result is a typed local");
    local["ty"] = ty;
}

/// Both boundaries must reject the forged module, and the shared validator's
/// reason must be the same on each.
fn assert_rejected_on_both_boundaries(encoded: Value, expected: &str) {
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
    let interpreted_reason = interpreted
        .message
        .rsplit_once(expected)
        .map(|(_, suffix)| suffix.to_owned())
        .unwrap_or_default();
    let native_reason = native
        .rsplit_once(expected)
        .map(|(_, suffix)| suffix.to_owned())
        .unwrap_or_default();
    assert_eq!(
        interpreted_reason, native_reason,
        "both boundaries should report the same validator reason"
    );
}

fn assert_accepted_on_both_boundaries(source: &str, stdout: &str) {
    let mir = lower_source_to_mir(source).expect("source should lower");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, stdout);
    emit_host_native_object(&mir).expect("the direct backend accepts the program");
}

const FROM_LIST: &str =
    "def main():\n    values = Array[int64].from_list([1, 2], [2])\n    print(values.len())\n";
const FROM_LIST_FLOAT: &str =
    "def main():\n    values = Array[float64].from_list([1.0, 2.0], [2])\n    print(values.len())\n";
const FULL: &str = "def main():\n    values = Array[int64].full([2], 5)\n    print(values.len())\n";
const ZEROS: &str =
    "def main():\n    values = Array[float64].zeros([2])\n    print(values.len())\n";

fn arg(value: Value) -> Value {
    json!({ "name": null, "value": value, "writeback_place": null })
}

#[test]
fn unforged_array_constructors_run_on_both_boundaries() {
    assert_accepted_on_both_boundaries(FROM_LIST, "2\n");
    assert_accepted_on_both_boundaries(FROM_LIST_FLOAT, "2\n");
    assert_accepted_on_both_boundaries(FULL, "2\n");
    assert_accepted_on_both_boundaries(ZEROS, "2\n");
}

#[test]
fn from_list_with_a_scalar_values_operand_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.from_list")[0]["value"] = json!({ "Int": 1 });
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` binds argument `values` to an operand that is not `list[int64]`",
    );
}

#[test]
fn from_list_with_a_scalar_shape_operand_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.from_list")[1]["value"] = json!({ "Int": 2 });
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` binds argument `shape` to an operand that is not `list[int64]`",
    );
}

#[test]
fn from_list_with_values_of_another_element_type_is_rejected_on_both_boundaries() {
    // Swapping the operands hands the `list[int64]` shape to the
    // `list[float64]` values slot.
    let mut encoded = encode(FROM_LIST_FLOAT);
    let main = function_mut(&mut encoded, "main");
    let args = call_args_mut(main, "Array.from_list");
    let shape = args[1]["value"].clone();
    args[0]["value"] = shape;
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` binds argument `values` to an operand that is not `list[float64]`",
    );
}

#[test]
fn from_list_with_an_unknown_argument_name_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.from_list")[1]["name"] = json!("sizes");
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` has unknown argument `sizes`",
    );
}

#[test]
fn from_list_binding_one_argument_twice_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    let args = call_args_mut(main, "Array.from_list");
    args[0]["name"] = json!("shape");
    args[1]["name"] = json!("shape");
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` binds argument `shape` more than once",
    );
}

#[test]
fn from_list_with_an_extra_argument_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.from_list").push(arg(json!({ "Int": 3 })));
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` has too many arguments",
    );
}

#[test]
fn from_list_missing_its_shape_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.from_list").pop();
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` omits argument `shape`",
    );
}

#[test]
fn from_list_with_a_writeback_place_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    let args = call_args_mut(main, "Array.from_list");
    let place = args[0]["value"]["Place"].clone();
    assert!(place.is_string(), "the values operand is a lowered place");
    args[0]["writeback_place"] = place;
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` supplies writeback for argument `values`",
    );
}

#[test]
fn from_list_with_a_non_array_result_type_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    retype_call_result(main, "Array.from_list", json!({ "Named": ["int64", []] }));
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` requires an `Array` result type with a numeric dtype",
    );
}

#[test]
fn from_list_with_an_unsupported_dtype_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    retype_call_result(
        main,
        "Array.from_list",
        json!({ "Named": ["Array", [{ "Named": ["str", []] }]] }),
    );
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` requires an `Array` result type with a numeric dtype",
    );
}

#[test]
fn from_list_whose_result_dtype_disagrees_with_its_values_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FROM_LIST);
    let main = function_mut(&mut encoded, "main");
    retype_call_result(
        main,
        "Array.from_list",
        json!({ "Named": ["Array", [{ "Named": ["float64", []] }]] }),
    );
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.from_list` in `main` binds argument `values` to an operand that is not `list[float64]`",
    );
}

#[test]
fn full_with_a_non_numeric_fill_value_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FULL);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.full")[1]["value"] = json!({ "String": "five" });
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.full` in `main` binds argument `value` to an operand that is not `int64`",
    );
}

#[test]
fn full_with_a_float_fill_value_for_an_integer_dtype_is_rejected_on_both_boundaries() {
    let mut encoded = encode(FULL);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.full")[1]["value"] = json!({ "Float": 5.5 });
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.full` in `main` binds argument `value` to an operand that is not `int64`",
    );
}

#[test]
fn zeros_with_a_scalar_shape_is_rejected_on_both_boundaries() {
    let mut encoded = encode(ZEROS);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.zeros")[0]["value"] = json!({ "Bool": true });
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.zeros` in `main` binds argument `shape` to an operand that is not `list[int64]`",
    );
}

#[test]
fn zeros_with_an_extra_argument_is_rejected_on_both_boundaries() {
    let mut encoded = encode(ZEROS);
    let main = function_mut(&mut encoded, "main");
    call_args_mut(main, "Array.zeros").push(arg(json!({ "Int": 1 })));
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR call to builtin `Array.zeros` in `main` has too many arguments",
    );
}
