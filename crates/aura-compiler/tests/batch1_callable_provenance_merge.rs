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

// Exact assignment/call checks now reject changed contracts before the
// downstream merge/binding paths these unchanged inputs previously exercised.
const CHANGED_CONTRACT: &str = "changes an authoritative callable contract";

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
    assert_rejected_on_both_boundaries(encoded, CHANGED_CONTRACT);
}

#[test]
fn keyword_only_contract_changes_are_rejected_before_candidates_merge() {
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
    assert_rejected_on_both_boundaries(encoded, "changes an authoritative callable contract");
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
    assert_rejected_on_both_boundaries(encoded, CHANGED_CONTRACT);
}

#[test]
fn default_contract_changes_are_rejected_before_candidates_merge() {
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
    assert_rejected_on_both_boundaries(encoded, "changes an authoritative callable contract");
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
    assert_rejected_on_both_boundaries(encoded, CHANGED_CONTRACT);
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
    assert_rejected_on_both_boundaries(encoded, CHANGED_CONTRACT);
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
    assert_rejected_on_both_boundaries(encoded, CHANGED_CONTRACT);
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
    assert_rejected_on_both_boundaries(encoded, CHANGED_CONTRACT);
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
fn keyword_only_contract_changes_on_either_side_are_rejected_before_merging() {
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
        assert_rejected_on_both_boundaries(encoded, "changes an authoritative callable contract");
    }
}

#[test]
fn merged_candidates_with_identical_tuple_contracts_call_on_both_boundaries() {
    let source = "def first(pair: (int64, str)) -> int64:\n    return pair[0] + 1\ndef second(pair: (int64, str)) -> int64:\n    return pair[0] + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool((7, \"a\")))\n";
    assert_accepted_on_both_boundaries(source, "9\n");
}

// ---------------------------------------------------------------------------
// Companions for the exact-contract closeout. The forged declaration changes
// above are now caught when the candidates enter typed storage. These tests
// take the same forgeries past that check by stripping every callable
// contract from `main`'s declared local metadata (a forged module may claim
// any local types it likes; the validator trusts only recorded identities),
// so the disagreement reaches the merge, poisons it, and the later call is
// refused on both boundaries with the same reason.
// ---------------------------------------------------------------------------

const POISONED_CALL: &str =
    "invalid MIR indirect call in `main` has no authoritative callable contract";

/// Replaces every `Function`/`Closure`/`Callable` contract inside `main`'s
/// declared local types with `int64`, leaving the assignment check no
/// declared callable positions to compare.
fn strip_declared_callable_metadata(function: &mut Value) {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(object) => {
                if ["Function", "Closure", "Callable"]
                    .iter()
                    .any(|kind| object.contains_key(*kind))
                {
                    *value = json!({ "Named": ["int64", []] });
                } else {
                    for child in object.values_mut() {
                        strip(child);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    strip(item);
                }
            }
            _ => {}
        }
    }
    for entry in function["local_types"].as_array_mut().unwrap() {
        strip(&mut entry["ty"]);
    }
}

/// Control first: stripping alone leaves the shared validator satisfied, so
/// the interpreter runs the program (the direct backend refuses the stripped
/// module later in codegen, where it needs the declared indirect function
/// type; that is a backend refusal after validation, not a silent
/// divergence). Then the forgery plus the stripping must poison the merge.
fn assert_stripped_merge_is_poisoned(source: &str, stdout: &str, forge: impl Fn(&mut Value)) {
    let mut control = encode(source);
    strip_declared_callable_metadata(function_mut(&mut control, "main"));
    let control: MirModule =
        serde_json::from_value(control).expect("stripped MIR should deserialize");
    assert_eq!(
        run_mir(&control)
            .expect("stripping the declared metadata alone passes shared validation")
            .stdout,
        stdout
    );
    let native = emit_host_native_object(&control)
        .expect_err("the direct backend needs the declared indirect function type");
    assert!(
        native.contains("expected an indirect function value"),
        "the stripped control is refused by codegen, not by the validator: {native}"
    );
    let mut encoded = encode(source);
    forge(&mut encoded);
    strip_declared_callable_metadata(function_mut(&mut encoded, "main"));
    assert_rejected_on_both_boundaries(encoded, POISONED_CALL);
}

#[test]
fn stripped_metadata_lets_a_slot_name_disagreement_poison_the_list_merge() {
    assert_stripped_merge_is_poisoned(&list_selection("first, second"), "9\n", |encoded| {
        forge_declaration_and_operands(
            encoded,
            "first",
            &|param| param["name"] = json!("other"),
            &|slot| slot["name"] = json!("other"),
        );
    });
}

#[test]
fn stripped_metadata_lets_a_keyword_only_disagreement_poison_the_list_merge() {
    let source = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(7))\n";
    assert_stripped_merge_is_poisoned(source, "9\n", |encoded| {
        forge_declaration_and_operands(
            encoded,
            "second",
            &|param| param["keyword_only"] = json!(true),
            &|slot| slot["keyword_only"] = json!(true),
        );
    });
}

#[test]
fn stripped_metadata_lets_a_keyword_only_and_name_disagreement_poison_the_list_merge() {
    let source = "def first(*, value: int64) -> int64:\n    return value + 1\ndef second(*, value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n";
    assert_stripped_merge_is_poisoned(source, "9\n", |encoded| {
        forge_declaration_and_operands(
            encoded,
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
    });
}

#[test]
fn stripped_metadata_lets_a_default_disagreement_poison_the_list_merge() {
    let source = "def first(value: int64 = 1) -> int64:\n    return value + 1\ndef second(value: int64 = 1) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n    print(tool())\n";
    assert_stripped_merge_is_poisoned(source, "9\n3\n", |encoded| {
        forge_declaration_and_operands(
            encoded,
            "second",
            &|param| param["default_function"] = Value::Null,
            &|slot| slot["has_default"] = json!(false),
        );
    });
}

#[test]
fn stripped_metadata_lets_a_default_and_name_disagreement_poison_the_list_merge() {
    let source = "def first(value: int64 = 1) -> int64:\n    return value + 1\ndef second(value: int64 = 1) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n";
    assert_stripped_merge_is_poisoned(source, "9\n", |encoded| {
        forge_declaration_and_operands(
            encoded,
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
    });
}

#[test]
fn stripped_metadata_lets_a_slot_name_disagreement_poison_the_dictionary_merge() {
    let source = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef key(name: own str) -> str:\n    return name\ndef main():\n    tools: dict[str, def(value: int64) -> int64] = {\"a\": first, \"b\": second}\n    tool = tools[key(\"b\")]\n    print(tool(value=7))\n";
    assert_stripped_merge_is_poisoned(source, "9\n", |encoded| {
        forge_declaration_and_operands(
            encoded,
            "second",
            &|param| param["name"] = json!("other"),
            &|slot| slot["name"] = json!("other"),
        );
    });
}

#[test]
fn stripped_metadata_lets_a_slot_name_disagreement_poison_the_control_flow_join() {
    let source = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    mut tool = first\n    if pick(1) == 1:\n        tool = second\n    print(tool(value=7))\n";
    assert_stripped_merge_is_poisoned(source, "9\n", |encoded| {
        forge_declaration_and_operands(
            encoded,
            "second",
            &|param| param["name"] = json!("other"),
            &|slot| slot["name"] = json!("other"),
        );
    });
}

#[test]
fn stripped_metadata_lets_a_slot_name_disagreement_poison_the_capture_free_lambda_merge() {
    let source = "def pick(index: int64) -> int64:\n    return index\ndef main():\n    tools: list[def(value: int64) -> int64] = [lambda value: value + 1, lambda value: value + 2]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n";
    assert_stripped_merge_is_poisoned(source, "9\n", |encoded| {
        let lambdas = lambda_functions(encoded, "main");
        assert_eq!(lambdas.len(), 2, "both lambdas lower to closure functions");
        forge_declaration_and_operands(
            encoded,
            &lambdas[1],
            &|param| param["name"] = json!("other"),
            &|slot| slot["name"] = json!("other"),
        );
    });
}

#[test]
fn stripped_metadata_lets_a_keyword_only_disagreement_on_either_side_poison_the_merge() {
    for (elements, stdout) in [("[first, second]", "9\n"), ("[second, first]", "8\n")] {
        let source = format!(
            "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = {elements}\n    tool = tools[pick(1)]\n    print(tool(7))\n"
        );
        assert_stripped_merge_is_poisoned(&source, stdout, |encoded| {
            forge_declaration_and_operands(
                encoded,
                "first",
                &|param| param["keyword_only"] = json!(true),
                &|slot| slot["keyword_only"] = json!(true),
            );
        });
    }
}

/// Forges one contract change consistently into both declarations, every
/// operand, and every signature in `main` (declared locals and literal
/// element types), so operand authentication, the typed assignment, and the
/// merge all accept it and only the indirect-call binder can refuse the call
/// shape the source wrote.
fn forge_consistently(
    encoded: &mut Value,
    declaration: &dyn Fn(&mut Value),
    slot: &dyn Fn(&mut Value),
) {
    fn visit(value: &mut Value, slot: &dyn Fn(&mut Value)) {
        match value {
            Value::Object(object) => {
                for kind in ["Function", "Closure", "Callable"] {
                    if let Some(first) = object
                        .get_mut(kind)
                        .and_then(|signature| signature.get_mut("params"))
                        .and_then(Value::as_array_mut)
                        .and_then(|params| params.first_mut())
                    {
                        if first["name"] == json!("value") {
                            slot(first);
                        }
                    }
                }
                for child in object.values_mut() {
                    visit(child, slot);
                }
            }
            Value::Array(items) => {
                for item in items {
                    visit(item, slot);
                }
            }
            _ => {}
        }
    }
    for name in ["first", "second"] {
        declaration(&mut function_mut(encoded, name)["params"][0]);
    }
    visit(function_mut(encoded, "main"), slot);
}

#[test]
fn consistently_keyword_only_contracts_refuse_a_positional_call_on_both_boundaries() {
    let source = "def first(value: int64) -> int64:\n    return value + 1\ndef second(value: int64) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(7))\n";
    let mut encoded = encode(source);
    forge_consistently(
        &mut encoded,
        &|param| param["keyword_only"] = json!(true),
        &|slot| slot["keyword_only"] = json!(true),
    );
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR indirect call from `main` has too many positional arguments",
    );
}

#[test]
fn consistently_required_contracts_refuse_an_omitted_argument_on_both_boundaries() {
    let source = "def first(value: int64 = 1) -> int64:\n    return value + 1\ndef second(value: int64 = 1) -> int64:\n    return value + 2\ndef pick(index: int64) -> int64:\n    return index\ndef main():\n    tools = [first, second]\n    tool = tools[pick(1)]\n    print(tool(value=7))\n    print(tool())\n";
    let mut encoded = encode(source);
    forge_consistently(
        &mut encoded,
        &|param| param["default_function"] = Value::Null,
        &|slot| slot["has_default"] = json!(false),
    );
    assert_rejected_on_both_boundaries(
        encoded,
        "invalid MIR indirect call from `main` omits required parameter 1 `value`",
    );
}
