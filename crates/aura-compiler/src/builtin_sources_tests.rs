use super::*;

fn path(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

#[test]
fn json_source_part_merges_into_the_host_namespace() {
    let host = builtin_module_namespace(&path("json")).expect("json is builtin");
    let part = source_part("json").expect("json has a source part");
    for name in part.functions.keys() {
        assert!(
            !host.functions.contains_key(name),
            "source function `{name}` must not shadow a host function"
        );
    }
    for name in part.enums.keys() {
        assert!(
            !host.enums.contains_key(name),
            "source enum `{name}` must not shadow a host enum"
        );
    }

    let merged = importable_builtin_namespace(&path("json")).expect("json is importable");
    for name in [
        "parse",
        "dumps",
        "encode",
        "decode",
        "decode_text",
        "schema",
    ] {
        assert!(merged.functions.contains_key(name), "missing `json.{name}`");
    }
    assert!(merged.traits.contains_key("Codec"));
    assert!(merged.classes.contains_key("DecodeError"));
    assert!(merged.enums.contains_key("DecodeErrorKind"));
    assert!(merged.enums.contains_key("Value"));
    assert!(
        !merged.functions.contains_key("describe"),
        "private source helpers stay unexported"
    );
}

#[test]
fn simd_is_a_source_module_of_lane_classes_and_operator_traits() {
    assert!(has_source_part(&path("simd")));
    assert!(crate::builtin_modules::is_builtin_root_module("simd"));
    let host = builtin_module_namespace(&path("simd")).expect("simd is builtin");
    assert!(host.functions.is_empty());
    let merged = importable_builtin_namespace(&path("simd")).expect("simd is importable");
    for (class, lanes) in [
        ("F64x2", 2),
        ("F64x4", 4),
        ("F32x4", 4),
        ("F32x8", 8),
        ("I64x2", 2),
        ("I64x4", 4),
    ] {
        let info = merged
            .classes
            .get(class)
            .unwrap_or_else(|| panic!("missing `simd.{class}`"));
        assert_eq!(info.decl.fields.len(), lanes, "{class}");
        assert!(info.decl.copy, "`simd.{class}` should be a Copy class");
    }
    for name in ["Sub", "Mul", "Div", "Neg"] {
        assert!(merged.traits.contains_key(name), "missing `simd.{name}`");
    }
}

#[test]
fn modules_without_a_source_part_bind_their_host_namespace() {
    assert!(!has_source_part(&path("math")));
    assert!(source_part("math").is_none());
    let math = importable_builtin_namespace(&path("math")).expect("math is builtin");
    assert!(math.traits.is_empty());
    assert!(importable_builtin_namespace(&path("missing")).is_none());
    assert!(!has_source_part(&["json".to_string(), "inner".to_string()]));
}

#[test]
fn the_registry_carries_the_source_parts_a_program_imports() {
    let parts = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
    let host = builtin_registry(&parts(&[]));
    assert!(!host["json"].traits.contains_key("Codec"));
    assert!(!host["iter"].classes.contains_key("ListIter"));
    let json = builtin_registry(&parts(&["json"]));
    assert!(json["json"].traits.contains_key("Codec"));
    assert_eq!(host.len(), json.len());
    // The parts behind operators and `for` come with any other part.
    assert!(json["iter"].classes.contains_key("ListIter"));
    // A module the program does not import adds nothing to its output.
    assert!(!json["regex"].classes.contains_key("Regex"));
    assert!(!json["tensor"].classes.contains_key("Tensor"));
    let both = builtin_registry(&parts(&["json", "tensor"]));
    assert!(both["tensor"].classes.contains_key("Tensor"));
}

#[test]
fn imported_source_parts_sees_module_and_from_imports() {
    let parts = |source: &str| {
        imported_source_parts(&crate::parse_source(source).expect("source parses"))
            .into_iter()
            .collect::<Vec<_>>()
    };
    assert_eq!(parts("import json\n"), ["json"]);
    assert_eq!(
        parts("from json import Codec\nimport regex\n"),
        ["json", "regex"]
    );
    assert!(parts("import math\n").is_empty());
    assert!(parts("from math import sqrt\n").is_empty());
}

#[test]
fn regex_source_part_adds_the_typed_api_to_the_host_searches() {
    let host = builtin_module_namespace(&path("regex")).expect("regex is builtin");
    let part = source_part("regex").expect("regex has a source part");
    for name in part.functions.keys() {
        assert!(
            !host.functions.contains_key(name),
            "source function `{name}` must not shadow a host function"
        );
    }
    let merged = importable_builtin_namespace(&path("regex")).expect("regex is importable");
    for name in ["compile", "escape", "__aura_is_match", "__aura_compile"] {
        assert!(
            merged.functions.contains_key(name),
            "missing `regex.{name}`"
        );
    }
    for name in ["Regex", "Match", "Captures"] {
        assert!(merged.classes.contains_key(name), "missing `regex.{name}`");
    }
    assert!(merged.enums.contains_key("Error"));
    assert!(merged.classes["Regex"].decl.copy, "a Regex is one integer");
    assert!(
        imported_source_parts(&crate::parse_source("import regex\n").expect("source parses"))
            .contains("regex")
    );
}

#[test]
fn time_source_part_adds_date_time_to_the_host_clocks() {
    let host = builtin_module_namespace(&path("time")).expect("time is builtin");
    let part = source_part("time").expect("time has a source part");
    for name in part.functions.keys() {
        assert!(
            !host.functions.contains_key(name),
            "source function `{name}` must not shadow a host function"
        );
    }
    let merged = importable_builtin_namespace(&path("time")).expect("time is importable");
    for name in [
        "now",
        "now_local",
        "monotonic",
        "utc",
        "from_unix",
        "from_unix_ms",
        "parse",
        "parse_rfc3339",
    ] {
        assert!(merged.functions.contains_key(name), "missing `time.{name}`");
    }
    for name in ["days_from_civil", "civil_year", "checked", "parsed"] {
        assert!(
            !merged.functions.contains_key(name),
            "`time.{name}` is a private helper"
        );
    }
    let date_time = &merged.classes["DateTime"];
    assert!(date_time.decl.copy, "a DateTime is three integers");
    assert!(date_time.decl.public);
    assert!(merged.enums.contains_key("Error"));
}
