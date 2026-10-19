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
fn modules_without_a_source_part_bind_their_host_namespace() {
    assert!(!has_source_part(&path("math")));
    assert!(source_part("math").is_none());
    let math = importable_builtin_namespace(&path("math")).expect("math is builtin");
    assert!(math.traits.is_empty());
    assert!(importable_builtin_namespace(&path("missing")).is_none());
    assert!(!has_source_part(&["json".to_string(), "inner".to_string()]));
}

#[test]
fn the_registry_carries_source_parts_only_on_request() {
    let host = builtin_registry(false);
    assert!(!host["json"].traits.contains_key("Codec"));
    let full = builtin_registry(true);
    assert!(full["json"].traits.contains_key("Codec"));
    assert_eq!(host.len(), full.len());
}

#[test]
fn imports_source_part_sees_module_and_from_imports() {
    let module = |source: &str| crate::parse_source(source).expect("source parses");
    assert!(imports_source_part(&module("import json\n")));
    assert!(imports_source_part(&module("from json import Codec\n")));
    assert!(!imports_source_part(&module("import math\n")));
    assert!(!imports_source_part(&module("from math import sqrt\n")));
}
