//! Forged-MIR regressions for complete-contract callable provenance merging
//! (C5, Q17 A, Q19 A). When a runtime selection may produce any of several
//! callables, the shared validator merges their authoritative identities by
//! their complete contracts (slot names, keyword-only boundaries, default
//! availability, closure contracts): a candidate that is a written
//! restriction of the others becomes the common contract, and any other
//! disagreement poisons the merged identity, so BOTH public boundaries
//! refuse a later call that relies on the disputed part of the contract with
//! the same reason, instead of one backend routing a named value to a slot
//! the selected declaration does not have.

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
}

fn assert_accepted_on_both_boundaries(source: &str, stdout: &str) {
    let mir = lower_source_to_mir(source).expect("source should lower");
    assert_eq!(run_mir(&mir).expect("the program runs").stdout, stdout);
    emit_host_native_object(&mir).expect("the direct backend accepts the program");
}

/// Applies `mutate` to the first parameter contract of every callable
/// signature (`Function`, `Closure`, or `Callable`) found in `value`.
fn mutate_first_signature_slot(value: &mut Value, mutate: &dyn Fn(&mut Value)) {
    for kind in ["Function", "Closure", "Callable"] {
        if let Some(slot) = value
            .get_mut(kind)
            .and_then(|signature| signature.get_mut("params"))
            .and_then(Value::as_array_mut)
            .and_then(|params| params.first_mut())
        {
            mutate(slot);
        }
    }
}

/// Rewrites every authenticated `Operand::Function` naming `function` so its
/// declared signature carries the forged first slot, everywhere in the
/// module.
fn forge_function_operands(value: &mut Value, function: &str, mutate: &dyn Fn(&mut Value)) {
    match value {
        Value::Object(object) => {
            if let Some(operand) = object.get_mut("Function") {
                if operand["name"] == json!(function) {
                    if let Some(signature) = operand.get_mut("signature") {
                        mutate_first_signature_slot(signature, mutate);
                    }
                }
            }
            for child in object.values_mut() {
                forge_function_operands(child, function, mutate);
            }
        }
        Value::Array(items) => {
            for item in items {
                forge_function_operands(item, function, mutate);
            }
        }
        _ => {}
    }
}

/// Forges the declaration of `function` and every authenticated operand
/// naming it consistently, so the forged contract passes the per-operand
/// declaration check and only the merge can refuse it.
fn forge_declaration_and_operands(
    encoded: &mut Value,
    function: &str,
    declaration: &dyn Fn(&mut Value),
    operand_slot: &dyn Fn(&mut Value),
) {
    declaration(&mut function_mut(encoded, function)["params"][0]);
    forge_function_operands(encoded, function, operand_slot);
}

const NO_CONTRACT: &str =
    "invalid MIR indirect call in `main` has no authoritative callable contract";

const FIRST_SECOND: &str = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\n";

fn list_selection(elements: &str) -> String {
    format!(
        "{FIRST_SECOND}def main():\n    tools: list[def(value: int64) -> int64] = [{elements}]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n"
    )
}

#[test]
fn runtime_selected_functions_with_one_complete_contract_call_on_both_boundaries() {
    assert_accepted_on_both_boundaries(&list_selection("first, second"), "9\n");
}

#[test]
fn merged_candidates_that_differ_in_a_slot_name_are_poisoned_on_both_boundaries() {
    let mut encoded = encode(&list_selection("first, second"));
    forge_declaration_and_operands(
        &mut encoded,
        "second",
        &|param| param["name"] = json!("other"),
        &|slot| slot["name"] = json!("other"),
    );
    assert_rejected_on_both_boundaries(encoded, NO_CONTRACT);
}

#[test]
fn merged_candidates_that_differ_in_a_keyword_only_boundary_keep_the_keyword_only_restriction() {
    // A keyword-only slot is a written restriction of the same named
    // positional slot, so the merge keeps it as the common contract: the
    // positional call every candidate accepted before the forgery is now
    // refused on both boundaries instead of one backend binding it by
    // position against a declaration that forbids that.
    let source = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(7))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
    let mut encoded = encode(source);
    forge_declaration_and_operands(
        &mut encoded,
        "second",
        &|param| param["keyword_only"] = json!(true),
        &|slot| slot["keyword_only"] = json!(true),
    );
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR indirect call from `main` has too many positional arguments",
    );
}

#[test]
fn merged_candidates_that_differ_in_a_keyword_only_boundary_and_name_are_poisoned_on_both_boundaries(
) {
    let source = "def first(*, value: int64) -> int64:\n    return value + 1\ndef second(*, value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
    let mut encoded = encode(source);
    forge_declaration_and_operands(
        &mut encoded,
        "second",
        &|param| {
            param["keyword_only"] = json!(false);
            param["name"] = json!("other");
        },
        &|slot| {
            slot["keyword_only"] = json!(false);
            slot["name"] = json!("other");
        },
    );
    assert_rejected_on_both_boundaries(encoded, NO_CONTRACT);
}

#[test]
fn merged_candidates_that_differ_in_default_availability_keep_the_dropped_default() {
    // The inferred element contract keeps both declarations' defaults. A
    // candidate without the default is a written restriction of the others,
    // so the merge drops the default: a call that omits the argument is
    // refused on both boundaries instead of one backend selecting a
    // declaration that has no default to bind.
    let source = "def first(value: int64 = 1) -> int64:\n    return value + 1\ndef second(value: int64 = 1) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n    print(tool())\n";
    assert_accepted_on_both_boundaries(source, "9\n3\n");
    let mut encoded = encode(source);
    forge_declaration_and_operands(
        &mut encoded,
        "second",
        &|param| param["default_function"] = Value::Null,
        &|slot| slot["has_default"] = json!(false),
    );
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR indirect call from `main` omits required parameter 1 `value`",
    );
}

#[test]
fn merged_candidates_that_differ_in_default_availability_and_name_are_poisoned_on_both_boundaries()
{
    let source = "def first(value: int64 = 1) -> int64:\n    return value + 1\ndef second(value: int64 = 1) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
    let mut encoded = encode(source);
    forge_declaration_and_operands(
        &mut encoded,
        "second",
        &|param| {
            param["default_function"] = Value::Null;
            param["name"] = json!("other");
        },
        &|slot| {
            slot["has_default"] = json!(false);
            slot["name"] = json!("other");
        },
    );
    assert_rejected_on_both_boundaries(encoded, NO_CONTRACT);
}

#[test]
fn merged_dictionary_values_that_differ_in_a_slot_name_are_poisoned_on_both_boundaries() {
    let source = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef key(name: own str) -> str:\n    return name\ndef main():\n    tools: dict[str, def(value: int64) -> int64] = {\"a\": first, \"b\": second}\n    tool = tools[key(\"b\")]\n    print(tool(value=7))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
    let mut encoded = encode(source);
    forge_declaration_and_operands(
        &mut encoded,
        "second",
        &|param| param["name"] = json!("other"),
        &|slot| slot["name"] = json!("other"),
    );
    assert_rejected_on_both_boundaries(encoded, NO_CONTRACT);
}

#[test]
fn control_flow_joins_that_differ_in_a_slot_name_are_poisoned_on_both_boundaries() {
    let source = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    mut tool = first\n    if pick(1) == 1:\n        tool = second\n    print(tool(value=7))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
    let mut encoded = encode(source);
    forge_declaration_and_operands(
        &mut encoded,
        "second",
        &|param| param["name"] = json!("other"),
        &|slot| slot["name"] = json!("other"),
    );
    assert_rejected_on_both_boundaries(encoded, NO_CONTRACT);
}

/// The lowered closure functions of `owner`'s lambdas, in source order.
fn lambda_functions(encoded: &Value, owner: &str) -> Vec<String> {
    let prefix = format!("{owner}::__lambda_");
    let mut names = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|function| function["name"].as_str().unwrap().to_owned())
        .filter(|name| name.starts_with(&prefix))
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn merged_capture_free_lambdas_that_differ_in_a_slot_name_are_poisoned_on_both_boundaries() {
    // A capture-free lambda is an authenticated function operand like a
    // named declaration.
    let source = "def pick(index: int64) -> int64:\n    return index\ndef main():\n    tools: list[def(value: int64) -> int64] = [lambda value: value + 1, lambda value: value + 2]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
    let mut encoded = encode(source);
    let lambdas = lambda_functions(&encoded, "main");
    assert_eq!(lambdas.len(), 2, "both lambdas lower to closure functions");
    forge_declaration_and_operands(
        &mut encoded,
        &lambdas[1],
        &|param| param["name"] = json!("other"),
        &|slot| slot["name"] = json!("other"),
    );
    assert_rejected_on_both_boundaries(encoded, NO_CONTRACT);
}

#[test]
fn packed_closures_that_differ_in_a_slot_name_are_refused_before_they_merge_on_both_boundaries() {
    // A capturing lambda is a closure literal whose rvalue signature is the
    // validator's authoritative contract; its declaration leads with the
    // hidden capture slots. Capturing closures merge only through an
    // explicitly packed common contract (here the `Tool` results `choose`
    // selects between), so a closure whose complete contract disagrees with
    // that contract is refused where it is packed, before any selection can
    // merge it, on both boundaries.
    let source = "type Tool = Callable[def(value: int64) -> int64]\ndef choose(flag: bool, left: own Tool, right: own Tool) -> Tool:\n    return left if flag else right\ndef make_offset(amount: int64) -> Tool:\n    offset = amount\n    return Tool(lambda value: value + offset)\ndef main():\n    chosen = choose(false, make_offset(1), make_offset(2))\n    print(chosen(value=7))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
    let mut encoded = encode(source);
    let lambdas = lambda_functions(&encoded, "make_offset");
    assert_eq!(
        lambdas.len(),
        1,
        "the lambda lowers to one closure function"
    );
    let forged = lambdas[0].clone();
    function_mut(&mut encoded, &forged)["params"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|param| param["name"] == json!("value"))
        .expect("the closure exposes its parameter")["name"] = json!("other");
    let owner = function_mut(&mut encoded, "make_offset");
    let mut rewritten = false;
    for block in owner["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction["Assign"]["value"]["Closure"]["function"] == json!(forged) {
                mutate_first_signature_slot(
                    &mut instruction["Assign"]["value"]["Closure"]["signature"],
                    &|slot| slot["name"] = json!("other"),
                );
                rewritten = true;
            }
        }
    }
    assert!(rewritten, "the forged closure literal should be found");
    assert_rejected_on_both_boundaries(encoded, "changes an authoritative callable contract");
}

#[test]
fn merged_candidates_that_differ_only_in_identity_keep_their_complete_contract() {
    // Distinct functions with one complete contract merge to that contract
    // without a name, so a named call through the selection still binds.
    let source = "def first(*, value: int64 = 1) -> int64:\n    return value + 1\ndef second(*, value: int64 = 1) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(0)]\n    print(tool(value=7))\n    print(tool())\n";
    assert_accepted_on_both_boundaries(source, "8\n2\n");
}

#[test]
fn merged_candidates_keep_a_keyword_only_restriction_from_either_side() {
    // The restriction may sit on either candidate; the merge keeps it as the
    // common contract regardless of element order.
    for (elements, restricted) in [("[first, second]", "first"), ("[second, first]", "first")] {
        let source = format!(
            "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = {elements}\n    tool = tools[pick(1)]\n    print(tool(7))\n"
        );
        let mut encoded = encode(&source);
        forge_declaration_and_operands(
            &mut encoded,
            restricted,
            &|param| param["keyword_only"] = json!(true),
            &|slot| slot["keyword_only"] = json!(true),
        );
        assert_rejected_on_both_boundaries(
            encoded,
            "invalid MIR indirect call from `main` has too many positional arguments",
        );
    }
}

#[test]
fn merged_candidates_with_identical_tuple_contracts_call_on_both_boundaries() {
    let source = "def first(pair: (int64, str)) -> int64:\n    return pair[0] + 1\ndef second(pair: (int64, str)) -> int64:\n    return pair[0] + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool((7, \"a\")))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
}
