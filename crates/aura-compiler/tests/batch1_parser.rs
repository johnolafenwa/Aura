//! Ratified Batch 1 grammar contracts, added before implementation.

use serde_json::{json, Value};

fn parsed(source: &str) -> Value {
    serde_json::to_value(aura_compiler::parse_source(source).expect("ratified syntax should parse"))
        .unwrap()
}

#[test]
fn union_precedence_and_grouping_preserve_callable_return_extent() {
    let module = parsed(
        "type Choice = def() -> int64 | str\n\
         type Reader = def() -> (int64 | str)\n\
         type Pair = (int64 | str, None)\n",
    );
    let choice = &module["items"][0]["TypeAlias"]["target"];
    assert_eq!(choice["members"].as_array().unwrap().len(), 2);
    assert_eq!(choice["members"][0]["return_type"]["name"], "int64");
    let reader = &module["items"][1]["TypeAlias"]["target"];
    assert_eq!(
        reader["return_type"]["members"].as_array().unwrap().len(),
        2
    );
    let pair = &module["items"][2]["TypeAlias"]["target"];
    assert_eq!(pair["elements"][0]["members"].as_array().unwrap().len(), 2);
    assert_eq!(pair["elements"][1]["name"], "None");
}

#[test]
fn aliases_preserve_visibility_generics_bounds_and_contextual_names() {
    let module = parsed(
        "public type Values[T: Eq] = list[T]\n\
         type = 1\n\
         def is(type: int64) -> int64:\n    return type\n",
    );
    let alias = &module["items"][0]["TypeAlias"];
    assert_eq!(alias["public"], true);
    assert_eq!(alias["name"], "Values");
    assert_eq!(alias["type_params"], json!(["T"]));
    assert_eq!(alias["type_param_bounds"]["T"][0]["name"], "Eq");
    assert_eq!(module["constants"][0]["name"], "type");
    assert_eq!(module["items"][1]["Function"]["name"], "is");
    assert_eq!(module["items"][1]["Function"]["params"][0]["name"], "type");
}

#[test]
fn callable_slots_preserve_names_default_promises_and_keyword_only_boundary() {
    let module = parsed(
        "def apply(callback: def(int64, label: str, *, retries: int32 = ...) -> bool):\n    pass\n\
         def configure(path: str, *, retries: int32 = 2):\n    pass\n",
    );
    let slots = &module["items"][0]["Function"]["params"][0]["ty"]["params"];
    assert!(slots[0]["name"].is_null());
    assert_eq!(slots[1]["name"], "label");
    assert_eq!(slots[2]["name"], "retries");
    assert_eq!(slots[2]["has_default"], true);
    assert_eq!(slots[2]["keyword_only"], true);
    assert_eq!(
        module["items"][1]["Function"]["params"][1]["keyword_only"],
        true
    );
}

#[test]
fn erased_callable_syntax_distinguishes_task_admission_and_invocation_mode() {
    let module = parsed(
        "type Update = Callable[mut def(value: int64) -> int64]\n\
         type Finish = TaskCallable[own def() -> str]\n",
    );
    let update = &module["items"][0]["TypeAlias"]["target"];
    assert_eq!(update["task"], false);
    assert_eq!(update["call_kind"], "BorrowMut");
    assert_eq!(update["signature"]["params"][0]["name"], "value");
    let finish = &module["items"][1]["TypeAlias"]["target"];
    assert_eq!(finish["task"], true);
    assert_eq!(finish["call_kind"], "Value");
}

#[test]
fn none_tests_compose_at_comparison_precedence() {
    let module = parsed("def test(value: int64) -> bool:\n    return value is not None and true\n");
    let result = &module["items"][0]["Function"]["body"][0]["Return"]["value"]["kind"];
    assert_eq!(result["Binary"]["op"], "And");
    assert_eq!(result["Binary"]["left"]["kind"]["IsNone"]["negated"], true);
}

#[test]
fn union_annotations_and_specializations_use_the_complete_type_grammar() {
    let module = parsed("def main():\n    value: int64 | str = 1\n    grouped: (int64 | str) | None = None\n    f: def(value: int64, *, label: str = ...) -> int64 = target\n    consume[int64 | str](value)\n");
    let body = &module["items"][0]["Function"]["body"];
    assert_eq!(body.as_array().unwrap().len(), 4);
}

#[test]
fn lambda_keyword_boundary_and_none_test_interpolation_keep_metadata() {
    let module = parsed(
        "def main():\n    call = lambda value, *, label: value\n    text = f\"{value is None}\"\n",
    );
    let encoded = module.to_string();
    assert!(encoded.contains("\"keyword_only\":true"));
    assert!(encoded.contains("\"IsNone\""));
}
