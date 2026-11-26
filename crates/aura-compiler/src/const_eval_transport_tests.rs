use crate::const_eval::FrozenValue;
use crate::{check_source, lower_to_mir, run_mir};

#[test]
fn typed_literals_and_union_payloads_survive_frozen_mir_transport() {
    let source = r#"const narrow: float32 = 16777216
const wide: float64 = 16777217
const tiny: float32 = 0.1
const tick = 2ms
const delay = tick + 3ms

def maybe(n: int64) -> int64 | None:
    if n == 0:
        return None
    return n

const optional = maybe(7)
const missing = maybe(0)
const pair = (true, delay, tiny, optional, missing)

def main():
    print(narrow)
    print(wide)
    print(delay)
    print(missing is None)
"#;
    let program = check_source(source).unwrap();
    let value = |name| &program.frozen_constants[&format!("<main>::{name}")];
    assert_eq!(value("tick"), &FrozenValue::Duration(2_000_000));
    assert_eq!(value("delay"), &FrozenValue::Duration(5_000_000));
    assert_eq!(
        value("tiny"),
        &FrozenValue::Float(f64::from(0.1_f32).to_bits())
    );
    assert!(matches!(value("missing"), FrozenValue::Union(_, _, v) if **v == FrozenValue::None));
    assert!(
        matches!(value("optional"), FrozenValue::Union(_, _, v) if matches!(**v, FrozenValue::Int(n) if n.as_i128() == Some(7)))
    );
    assert_eq!(
        FrozenValue::freeze(&value("pair").thaw()).as_ref(),
        Some(value("pair"))
    );
    let encoded = serde_json::to_vec(&lower_to_mir(&program)).unwrap();
    let restored = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(
        run_mir(&restored).unwrap().stdout,
        "16777216.0\n16777217.0\n5ms\ntrue\n"
    );
}

#[test]
fn a_generic_enum_payloads_display_and_omitted_defaults_keep_their_effects() {
    let source = r#"class Stamp:
    label: str

impl Display for Stamp:
    def display(self) -> str:
        print("display")
        return self.label.clone()

enum Entry[T]:
    Found(T)

def typed_entry() -> Entry[Stamp]:
    return Entry.Found(Stamp(label="x"))

def side() -> int64:
    print("default")
    return 11

def capture(*, n: int64 = side()) -> int64:
    return n

const silent = capture(n=7)
with_default = capture()
via_binding = with_default + 1
rendered = f"{(typed_entry(), 3)}"

def main():
    print(silent)
    print(via_binding)
"#;
    let program = check_source(source).unwrap();
    assert!(
        matches!(program.frozen_constants["<main>::silent"], FrozenValue::Int(n) if n.as_i128() == Some(7))
    );
    for name in ["with_default", "via_binding", "rendered"] {
        assert!(!program
            .frozen_constants
            .contains_key(&format!("<main>::{name}")));
    }
    assert_eq!(
        run_mir(&lower_to_mir(&program)).unwrap().stdout,
        "default\ndisplay\n7\n12\n"
    );
}

#[test]
fn string_transformations_format_widths_and_map_codecs_fold_together() {
    let source = r#"import json
import toml

def label(s: str, width: int64) -> str:
    parts = s.to_upper().replace("O", "xy").split(" ")
    text = "|".join(parts)
    return f"[{text:*>12}]/{width:04}"

const title = label("one two", 7)
const meta = toml.parse_string_map("board = \"pico\"")
const encoded = json.stringify_map({"board": "pico"})
const ok = toml.is_valid("reader = true")

def main():
    print(title)
    print(meta)
    print(encoded)
    print(ok)
"#;
    let program = check_source(source).unwrap();
    for name in ["title", "meta", "encoded", "ok"] {
        assert!(
            program
                .frozen_constants
                .contains_key(&format!("<main>::{name}")),
            "{name}"
        );
    }
    assert_eq!(
        run_mir(&lower_to_mir(&program)).unwrap().stdout,
        "[***xyNE|TWxy]/0007\nResult.Ok({board: pico})\nResult.Ok({\"board\":\"pico\"})\ntrue\n"
    );
}

#[test]
fn clocks_tasks_and_foreign_calls_only_initialize_at_runtime() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/packages/ffi_getpid/src/main.au");
    let source = r#"import time

extern "C" def runtime_counter() -> int64

def sample_clock() -> int64:
    _ = time.monotonic()
    return 1

def worker() -> int64:
    return 2

def start_worker() -> int64:
    with TaskGroup() as tasks:
        tasks.start(worker)
    return 3

def read_foreign() -> int64:
    return runtime_counter()

clock_value = sample_clock()
task_value = start_worker()
foreign_value = read_foreign()
"#;
    let program = crate::check_path_with_source_without_lockfile(&path, source).unwrap();
    for name in ["clock_value", "task_value", "foreign_value"] {
        assert!(!program
            .frozen_constants
            .contains_key(&format!("{}::{name}", program.module_name)));
        let required = source.replace(&format!("{name} ="), &format!("const {name} ="));
        let error = crate::check_path_with_source_without_lockfile(&path, &required).unwrap_err();
        assert_eq!(error.code, "AU3025");
        assert!(error.message.contains(name));
    }
}
