//! Union injection contracts, installed before semantic or MIR implementation.
use aura_compiler::{check_source, parse_source};
use std::path::Path;

fn accepts_fixture(name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/run-pass")
        .join(format!("union_injection_{name}.au"));
    let source = std::fs::read_to_string(path).unwrap();
    parse_source(&source).expect("fixture uses existing ratified syntax");
    check_source(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
    let output = aura_compiler::run_source(&source)
        .unwrap_or_else(|error| panic!("{name} MIR runtime: {error}"));
    assert_eq!(output.stdout, "42\n", "{name}");
}

macro_rules! accepts {
    ($($name:ident),+ $(,)?) => { $(#[test] fn $name() { accepts_fixture(stringify!($name)); })+ };
}
accepts!(
    binding,
    parameter,
    return_value,
    explicit_generic,
    list,
    dict,
    tuple,
    field,
    enum_payload,
    group,
    conditional,
    match_expression,
    lambda_result,
    none,
    cast,
    typed_local,
    bool_value,
    negative_float
);

#[test]
fn integer_literal_with_two_numeric_members_is_ambiguous() {
    let error = check_source("def main():\n    value: int64 | float64 = 1\n")
        .expect_err("two contextual literal matches");
    assert_eq!(error.code, "AU2011", "{error}");
    // The literal starts after the annotation, equals sign, and spaces.
    assert_eq!(error.span, Some(aura_compiler::Span::new(2, 30)));
}

#[test]
fn absent_member_reports_the_union_boundary() {
    let error = check_source("def main():\n    value: int64 | str = true\n")
        .expect_err("bool is not an integer member");
    assert_eq!(error.code, "AU2010", "{error}");
    assert_eq!(error.message, "'bool' is not a member of 'int64 | str'");
}

#[test]
fn a_typed_integer_does_not_promote_into_a_float_member() {
    let error =
        check_source("def main():\n    source: int64 = 1\n    value: float64 | str = source\n")
            .expect_err("typed numeric values do not promote for injection");
    assert_eq!(error.code, "AU2010", "{error}");
}

#[test]
fn a_mutable_member_place_cannot_be_retagged_as_a_union() {
    let source = "def replace(value: mut (int64 | str)):\n    value = \"text\"\ndef main():\n    mut value: int64 = 1\n    replace(value=value)\n";
    let error = check_source(source).expect_err("mutable type contract is invariant");
    assert!(
        matches!(error.code.as_str(), "AU2002" | "AU2010" | "AU3003"),
        "{error}"
    );
}

#[test]
fn union_returns_still_require_an_explicit_none_expression() {
    for source in [
        "def make() -> int64 | None:\n    return\n",
        "def make() -> int64 | None:\n    pass\n",
    ] {
        assert!(
            check_source(source).is_err(),
            "optional return must be explicit"
        );
    }
}

#[test]
fn typed_collections_do_not_convert_element_by_element() {
    let error = check_source(
        "def main():\n    source: list[int64] = [1]\n    value: list[int64 | str] = source\n",
    )
    .expect_err("collection storage is invariant");
    assert_eq!(error.code, "AU2002", "{error}");
}

#[test]
fn an_outer_union_does_not_supply_inferred_call_arguments() {
    let source = "def identity[T](value: T) -> T:\n    return value\ndef main():\n    value: list[int64] | str = identity(value=[])\n";
    assert!(
        check_source(source).is_err(),
        "the empty list has no argument context"
    );
}

#[test]
fn whole_collection_literals_probe_members_transactionally() {
    check_source("def main():\n    value: list[int64] | str = [42]\n").unwrap();
    let error = check_source("def main():\n    value: list[int64] | list[float64] = [42]\n")
        .expect_err("two literal interpretations");
    assert_eq!(error.code, "AU2011", "{error}");
}

#[test]
fn shared_noncopy_member_cannot_be_silently_owned() {
    let source = "def pack(value: list[int64]) -> list[int64] | None:\n    return value\n";
    assert!(
        check_source(source).is_err(),
        "injection does not manufacture ownership"
    );
}

#[test]
fn mir_pins_dense_tags_in_normalized_order() {
    let program = check_source("def main():\n    a: str | int64 | None = 42\n    b: None | str | int64 = \"text\"\n    c: int64 | None | str = None\n").unwrap();
    let mir = aura_compiler::mir::lower(&program);
    let encoded = serde_json::to_value(
        mir.functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap(),
    )
    .unwrap();
    fn injections(value: &serde_json::Value, found: &mut Vec<serde_json::Value>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(injection) = map.get("UnionInject") {
                    found.push(injection.clone());
                }
                for value in map.values() {
                    injections(value, found);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    injections(value, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    injections(&encoded, &mut found);
    assert_eq!(found.len(), 3, "one injection per value: {encoded}");
    for (index, injection) in found.iter().enumerate() {
        assert_eq!(injection["member_index"], index);
    }
}

#[test]
fn match_result_preserves_the_selected_tag_and_payload() {
    let mir = aura_compiler::lower_source_to_mir("def make() -> int64 | str:\n    return match true:\n        case true: 42\n        case false: \"text\"\n").unwrap();
    let output = aura_compiler::run_mir_entry(&mir, Some("make"), None, Vec::new()).unwrap();
    let aura_compiler::Value::Union(value) = output.value else {
        panic!("match result lost union representation: {:?}", output.value);
    };
    assert_eq!(value.member_index, 0);
    assert_eq!(value.payload.render(), "42");
}

#[test]
fn result_call_is_evaluated_once_before_injection() {
    let source = include_str!("fixtures/run-pass/union_injection_once.au");
    let output = aura_compiler::run_source(source).unwrap();
    assert_eq!(output.stdout, "41\n42\n");
}

#[test]
fn literal_without_a_viable_member_reports_injection_failure() {
    for source in [
        "def main():\n    value: int64 | str = []\n",
        "def main():\n    value: int8 | uint8 = 256\n",
    ] {
        let error = check_source(source).unwrap_err();
        assert_eq!(error.code, "AU2010", "{error}");
    }
}

#[test]
fn negative_float_literals_receive_member_context() {
    check_source("def main():\n    value: float32 | str = -1.0\n").unwrap();
}

#[test]
fn imported_bodies_retain_injection_decisions() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/run-pass/union_injection_imported.au");
    let mir = aura_compiler::lower_path_to_mir(&path).unwrap();
    let function = mir
        .functions
        .iter()
        .find(|f| f.name.ends_with("::make"))
        .unwrap();
    let encoded = serde_json::to_string(function).unwrap();
    assert!(
        encoded.contains("UnionInject"),
        "imported union body lost checked injection: {encoded}"
    );
    assert_eq!(aura_compiler::run_path(&path).unwrap().stdout, "42\n");
}

#[test]
fn imported_nominal_payload_uses_its_defining_type() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/run-pass/union_injection_imported_nominal.au");
    // Imported nominal rendering already uses the defining module path.
    assert_eq!(
        aura_compiler::run_path(&path).unwrap().stdout,
        "union_injection_support.values.Packet(value=43)\n"
    );
    let mir = aura_compiler::lower_path_to_mir(&path).unwrap();
    aura_compiler::emit_host_native_object(&mir).unwrap();
}
