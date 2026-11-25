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
    for name in ["compile", "escape"] {
        assert!(
            merged.functions.contains_key(name),
            "missing `regex.{name}`"
        );
    }
    for name in ["__aura_is_match", "__aura_compile"] {
        assert!(
            !merged.functions.contains_key(name),
            "internal `regex.{name}` should stay private"
        );
        assert!(
            merged.all_functions.contains_key(name),
            "missing internal `regex.{name}` in all_functions"
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
fn image_source_part_adds_the_typed_api_to_the_host_codecs() {
    let host = builtin_module_namespace(&path("image")).expect("image is builtin");
    let part = source_part("image").expect("image has a source part");
    for name in part.functions.keys() {
        assert!(
            !host.functions.contains_key(name),
            "source function `{name}` must not shadow a host function"
        );
    }
    let merged = importable_builtin_namespace(&path("image")).expect("image is importable");
    for name in [
        "read",
        "write",
        "write_as",
        "decode",
        "decode_up_to",
        "info",
        "encode",
        "encode_png",
        "encode_jpeg",
        "encode_ppm",
        "format_of",
    ] {
        assert!(
            merged.functions.contains_key(name),
            "missing `image.{name}`"
        );
    }
    assert!(
        !merged.functions.contains_key("__aura_decode"),
        "internal `image.__aura_decode` should stay private"
    );
    assert!(
        merged.all_functions.contains_key("__aura_decode"),
        "missing internal `image.__aura_decode` in all_functions"
    );
    for name in ["Image", "Info"] {
        assert!(merged.classes.contains_key(name), "missing `image.{name}`");
    }
    for name in ["Error", "Format", "Filter"] {
        assert!(merged.enums.contains_key(name), "missing `image.{name}`");
    }
    // A program that imports `json` carries nothing of `image`.
    let parts = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
    let json = builtin_registry(&parts(&["json"]));
    assert!(!json["image"].classes.contains_key("Image"));
    let image = builtin_registry(&parts(&["image"]));
    assert!(image["image"].classes.contains_key("Image"));
    assert!(!image["tensor"].classes.contains_key("Tensor"));
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

#[test]
fn internal_builtin_helpers_and_handles_are_private_to_user_code() {
    let rejected = [
        (
            "sync.__aura_cell_new",
            "import sync\ndef main():\n    _ = sync.__aura_cell_new[int64](1)\n",
        ),
        (
            "sync.Cell type",
            "import sync\ndef take_cell(c: sync.Cell) -> None:\n    pass\n",
        ),
        (
            "sync.Atom type",
            "import sync\ndef take_atom(a: sync.Atom) -> None:\n    pass\n",
        ),
        (
            "Locked.__aura_take",
            "import sync\ndef main():\n    m = sync.Mutex[int64](1)\n    locked = m.lock()\n    _ = locked.__aura_take()\n",
        ),
        (
            "Locked.__aura_put",
            "import sync\ndef main():\n    m = sync.Mutex[int64](1)\n    locked = m.lock()\n    locked.__aura_put(2)\n",
        ),
        (
            "iter.__aura_generator_yield",
            "import iter\ndef main():\n    _ = iter.__aura_generator_yield[int64](1)\n",
        ),
        (
            "iter.__aura_generator_take",
            "import iter\ndef bad(h: iter.GeneratorHandle) -> int64:\n    return iter.__aura_generator_take[int64](h)\n",
        ),
        (
            "from sync import __aura_cell_new",
            "from sync import __aura_cell_new\ndef main():\n    _ = __aura_cell_new[int64](1)\n",
        ),
        (
            "tensor.__aura_filled",
            "import tensor\ndef main():\n    _ = tensor.__aura_filled([1], 2, 0, 1, 0)\n",
        ),
        (
            "tensor.__aura_node_op",
            "import tensor\ndef main():\n    t = tensor.Tensor[float32].zeros([2], tensor.Device.cpu())\n    _ = tensor.__aura_node_op(t, 6, list[int64]())\n",
        ),
        (
            "tensor.Node type",
            "import tensor\ndef take_node(n: tensor.Node) -> None:\n    pass\n",
        ),
        (
            "regex.__aura_compile",
            "import regex\ndef main():\n    _ = regex.__aura_compile(\"a\")\n",
        ),
        (
            "image.__aura_decode",
            "import image\ndef main():\n    _ = image.__aura_decode(bytes())\n",
        ),
    ];
    for (label, source) in rejected {
        assert!(
            crate::check_source(source).is_err(),
            "expected `{label}` to be rejected in user code"
        );
    }

    // Standard public APIs backed by those internal helpers continue to work.
    let output = crate::run_source(
        r#"
import iter
import sync
from tensor import Device, Tensor

def count_two() -> iter.Generator[int64]:
    yield 10
    yield 20

def main():
    m = sync.Mutex[int64](5)
    with v = m.lock():
        v += 7
    mut total = 0
    for item in count_two():
        total += item
    t = Tensor[float32].zeros([2], Device.cpu())
    with v = m.lock():
        print(f"{v}:{total}:{t.len()}")
"#,
    )
    .expect("public sync, iter, and tensor APIs should keep working");
    assert_eq!(output.stdout, "12:30:2\n");
}
