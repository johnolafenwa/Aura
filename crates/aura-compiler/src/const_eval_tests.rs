use super::*;
use crate::{check_source, lower_source_to_mir, run_source};

#[test]
fn frozen_module_constants_retain_types_and_iteration_order() {
    let source = include_str!("../tests/fixtures/run-pass/frozen_module_values.au");
    let mir = lower_source_to_mir(source).unwrap();
    for constant in &mir.constants {
        let frozen = constant.frozen.as_ref().expect(&constant.key);
        assert_eq!(FrozenValue::freeze(&frozen.thaw()).as_ref(), Some(frozen));
    }
    assert_eq!(
        run_source(source).unwrap().stdout,
        include_str!("../tests/fixtures/run-pass/frozen_module_values.stdout")
    );
}

#[test]
fn required_initializers_explain_impurity_and_keep_ordinary_effects_at_runtime() {
    let source = include_str!("../tests/fixtures/check-fail/required_effectful_constant.au");
    let error = check_source(source).unwrap_err();
    assert_eq!(error.code, "AU3025");
    assert_eq!(
        run_source(&(source.replace("const token", "token") + "\ndef main():\n    print(token)\n"))
            .unwrap()
            .stdout,
        "effect\n1\n"
    );
}

#[test]
fn constant_diagnostics_preserve_traps_and_bound_nested_work() {
    let trap = check_source("const a = f()\ndef f() -> int64:\n    return 1 // 0\n").unwrap_err();
    assert_eq!(trap.code, "AU3026");
    assert!(trap.message.contains("division by zero"));
    assert_eq!(trap.call_frames[0].function, "f");
    assert_eq!(trap.span.unwrap().line, 3);
    let steps = check_source("const a = f()\ndef f() -> int64:\n    mut i: int64 = 0\n    while i < 100000:\n        i += 1\n    return i\n").unwrap_err();
    assert_eq!(steps.code, "AU3027");
    let bytes = check_source("const a = \"x\".repeat(8388609)\n").unwrap_err();
    assert_eq!(bytes.code, "AU3028");
}

#[test]
fn computed_scalars_round_at_the_target_width_and_survive_serialization() {
    let source = r#"import math
const big: int128 = (1 as int128) << 90
const neg: int64 = 1 - 15
const word: uintsize = (1 as uintsize) << 63
const rounded: float32 = (16777216.0 as float32) + 1.0
const sine = math.sin(1.0)
const infinite = math.inf * 2.0
const nan = math.nan + 0.0

def main():
    print(neg)
"#;
    let program = check_source(source).unwrap();
    let value = |key: &str| &program.frozen_constants[&format!("<main>::{key}")];
    assert_eq!(
        value("rounded"),
        &FrozenValue::Float(16_777_216.0f64.to_bits())
    );
    assert_eq!(value("sine"), &FrozenValue::Float(0x3fea_ed54_8f09_0cee));
    assert_eq!(value("nan"), &FrozenValue::Float(f64::NAN.to_bits()));
    let mir = crate::lower_to_mir(&program);
    let serialized = serde_json::to_vec(&mir).unwrap();
    let restored: mir::MirModule = serde_json::from_slice(&serialized).unwrap();
    assert_eq!(crate::run_mir(&restored).unwrap().stdout, "-14\n");
}

#[test]
fn effects_hidden_behind_a_callback_or_after_a_trap_preserve_runtime_initialization() {
    let source = r#"def apply(f: def(int32) -> int32) -> int32:
    return f(2)

def clean(n: int32) -> int32:
    return n + 1

def observe(n: int32) -> int32:
    print(n)
    return n

def later_effect() -> int32:
    n: int32 = 1 // 0
    print(n)
    return n

const folded = apply(clean)
dynamic = apply(observe)
runtime_trap = later_effect()
"#;
    let program = check_source(source).unwrap();
    assert!(program.frozen_constants.contains_key("<main>::folded"));
    assert!(!program.frozen_constants.contains_key("<main>::dynamic"));
    assert!(!program
        .frozen_constants
        .contains_key("<main>::runtime_trap"));
}

#[test]
fn lexical_paths_follow_the_selected_targets_path_rules() {
    let source = r#"import path
const joined = path.join("one", "two")
const parent = path.parent("one/two.name")
const leaf = path.file_name("one/two.name")
const ext = path.extension("one/two.name")
const absolute = path.is_absolute("C:/data")
"#;
    for (target, separator, absolute) in [
        ("x86_64-unknown-linux-gnu", "/", false),
        ("x86_64-pc-windows-msvc", "\\", true),
    ] {
        crate::with_build_config(
            crate::BuildConfig::for_target(target, false).unwrap(),
            || {
                let program = check_source(source).unwrap();
                assert_eq!(
                    program.frozen_constants["<main>::joined"],
                    FrozenValue::Str(format!("one{separator}two"))
                );
                assert_eq!(
                    program.frozen_constants["<main>::absolute"],
                    FrozenValue::Bool(absolute)
                );
                for (name, expected) in [("parent", "one"), ("leaf", "two.name"), ("ext", "name")] {
                    let FrozenValue::Union(_, _, value) =
                        &program.frozen_constants[&format!("<main>::{name}")]
                    else {
                        panic!("expected an optional path component")
                    };
                    assert_eq!(value.as_ref(), &FrozenValue::Str(expected.to_string()));
                }
            },
        );
    }
}

#[test]
fn deterministic_codecs_and_big_integers_share_the_constant_budget() {
    let source = r#"import bytes
import gzip
import regex
from numbers import BigInt
const packed = gzip.compress(b"abc")
const decoded = gzip.decompress(packed)
const escaped = regex.escape("a.b")
const big = BigInt.from_int(2).pow(130).to_radix(16)
"#;
    let program = check_source(source).unwrap();
    assert!(matches!(
        program.frozen_constants["<main>::decoded"],
        FrozenValue::Enum(..)
    ));
    assert_eq!(
        program.frozen_constants["<main>::escaped"],
        FrozenValue::Str(r"a\.b".to_string())
    );
    assert_eq!(
        program.frozen_constants["<main>::big"],
        FrozenValue::Str(format!("4{}", "0".repeat(32)))
    );
    let error = check_source(
        "from numbers import BigInt\nconst huge = BigInt.from_int(1).shift_left(1000000000)\n",
    )
    .unwrap_err();
    assert_eq!(error.code, "AU3028");
}

#[test]
fn unused_defaults_and_unrelated_display_implementations_are_not_effects() {
    let source = r#"class Tag:
    n: int64

impl Display for Tag:
    def display(self) -> str:
        print("effect")
        return "tag"

def observe() -> int64:
    print("default")
    return 0

def f(n: int64 = observe()) -> int64:
    return n

const selected = f(9)
const message = f"answer {selected}"
dynamic = f()
"#;
    let program = check_source(source).unwrap();
    assert_eq!(
        program.frozen_constants["<main>::message"],
        FrozenValue::Str("answer 9".to_string())
    );
    assert!(!program.frozen_constants.contains_key("<main>::dynamic"));
}

#[test]
fn evaluation_diagnostics_match_the_documented_source_spans_and_call_frames() {
    macro_rules! fixture {
        ($name:literal) => {{
            let path = concat!(
                "crates/aura-compiler/tests/fixtures/check-fail/",
                $name,
                ".au"
            );
            let source = include_str!(concat!("../tests/fixtures/check-fail/", $name, ".au"));
            let expected = include_str!(concat!("../tests/fixtures/check-fail/", $name, ".diag"));
            let rendered = check_source(source)
                .unwrap_err()
                .render_with_source(path, source);
            assert_eq!(rendered.trim_end(), expected.trim_end());
        }};
    }
    fixture!("module_constant_indirect_reentry");
    fixture!("module_constant_initializer_failure");
    fixture!("required_effectful_constant");
    fixture!("compile_time_step_limit");
    fixture!("compile_time_memory_limit");
}

#[test]
fn intermediate_arrays_are_bounded_before_allocating_their_target_shape() {
    let source = r#"def from_array() -> int64:
    mut numbers = Array[int64].zeros([2, 2])
    numbers[1, 1] = 42
    return numbers[1, 1]

const answer = from_array()
"#;
    let program = check_source(source).unwrap();
    assert!(
        matches!(program.frozen_constants["<main>::answer"], FrozenValue::Int(n) if n.as_i128() == Some(42))
    );
    let error = check_source("const huge = Array[int64].zeros([2000000]).len()\n").unwrap_err();
    assert_eq!(error.code, "AU3028");
}

#[test]
fn growing_codec_results_and_repeated_member_calls_stop_at_the_allowance() {
    let codec = "import bytes\nconst input = \"a\".repeat(3000000).to_bytes()\nconst huge = bytes.hex_encode(input)\n";
    // A compact encoded result and its input count as separate allocations in one initializer.
    let error = check_source(&codec.replace(
        "bytes.hex_encode(input)",
        "bytes.hex_encode(input).repeat(2)",
    ))
    .unwrap_err();
    assert_eq!(error.code, "AU3028");
    let error = check_source("const data = f()\ndef f() -> int64:\n    mut i: int64 = 0\n    while i < 20:\n        text = \"a\".repeat(1000000)\n        i += text.byte_len() // 1000000\n    return i\n").unwrap_err();
    assert_eq!(error.code, "AU3028");
}

#[test]
fn bounded_image_operations_run_on_the_portable_codec_path() {
    let program = check_source(
        r#"import image
const pixel = image.Image.filled(2, 1, 3, 7).resize(3, 2).flip_horizontal().get(2, 1, 0)
const image_file = image.encode_png(image.Image.filled(2, 1, 1, 7))
"#,
    )
    .unwrap();
    assert!(
        matches!(program.frozen_constants["<main>::pixel"], FrozenValue::Int(n) if n.as_i128() == Some(7))
    );
    assert!(matches!(
        program.frozen_constants["<main>::image_file"],
        FrozenValue::Enum(..)
    ));
    let error = check_source("import image\nconst enormous = image.Image.filled(1, 1, 1, 7).resize(50000, 50000).width\n").unwrap_err();
    assert_eq!(error.code, "AU3028");
}

#[test]
fn formatting_a_nested_object_considers_only_its_possible_display_methods() {
    let source = r#"class Label:
    n: int64

class Noisy:
    n: int64

impl Display for Label:
    def display(self) -> str:
        return f"n={self.n}"

impl Display for Noisy:
    def display(self) -> str:
        print("effect")
        return "noisy"

class Bucket:
    tags: list[Label]

const title = f"{Bucket(tags=[Label(n=7)])}"
dynamic = f"{[Noisy(n=1)]}"
"#;
    let program = check_source(source).unwrap();
    assert!(
        matches!(&program.frozen_constants["<main>::title"], FrozenValue::Str(text) if text.contains("n=7"))
    );
    assert!(!program.frozen_constants.contains_key("<main>::dynamic"));
}

#[test]
fn source_collections_freeze_their_portable_storage_and_json_parses_on_the_evaluator() {
    let source = r#"from collections import Deque
import json

def queue() -> Deque[int64]:
    mut waiting = Deque[int64]()
    waiting.push_back(8)
    waiting.push_front(9)
    return waiting

const q = queue()
const parsed = json.parse("[1, 2]")
const invalid = json.parse("[1,]")

def main():
    print(q.len())
"#;
    let program = check_source(source).unwrap();
    assert!(matches!(
        &program.frozen_constants["<main>::q"],
        FrozenValue::Class(..)
    ));
    assert!(
        matches!(&program.frozen_constants["<main>::parsed"], FrozenValue::Enum(_,variant,_) if variant == "Ok")
    );
    assert!(
        matches!(&program.frozen_constants["<main>::invalid"], FrozenValue::Enum(_,variant,_) if variant == "Err")
    );
    assert_eq!(run_source(source).unwrap().stdout, "2\n");
}

#[test]
fn regex_searches_can_use_local_patterns_without_freezing_a_handle() {
    let source = r#"import regex

def count() -> int64:
    match regex.compile("xy+"):
        case Result.Ok(pattern):
            return pattern.count("xyy xy x")
        case Result.Err(_):
            return -1

const n = count()
handle = regex.compile("value")
"#;
    let program = check_source(source).unwrap();
    assert!(
        matches!(program.frozen_constants["<main>::n"], FrozenValue::Int(n) if n.as_i128() == Some(2))
    );
    assert!(!program.frozen_constants.contains_key("<main>::handle"));
    assert_eq!(
        check_source(&source.replace("handle =", "const handle ="))
            .unwrap_err()
            .code,
        "AU3025"
    );
}

#[test]
fn decorator_bindings_and_unsupported_results_do_not_request_evaluation_mir() {
    let mut program =
        check_source(include_str!("../tests/fixtures/run-pass/decorators.au")).unwrap();
    assert!(!program.constant_init_plan.is_empty());
    assert!(evaluation_mir(&mut program).unwrap().is_none());

    let mixed = r#"type Step = Callable[def(n: int64) -> int64]

def delayed() -> Step:
    assert false, "startup"
    return Step(lambda n: n)

def make() -> Step:
    return Step(lambda n: n + 2)

pending = delayed()
const folded = make()(40)
"#;
    let mut program = check_source(mixed).unwrap();
    assert!(
        matches!(&program.frozen_constants["<main>::folded"], FrozenValue::Int(n) if n.as_i128() == Some(42))
    );
    let module = evaluation_mir(&mut program).unwrap().unwrap();
    assert_eq!(module.constants.len(), 1);
    assert_eq!(module.constants[0].key, "<main>::folded");
    assert!(!module
        .functions
        .iter()
        .any(|f| f.name == "__aura_const_init::<main>::pending"));
    assert_eq!(
        check_source(&mixed.replace("pending =", "const pending ="))
            .unwrap_err()
            .code,
        "AU3025"
    );
    assert!(run_source(mixed).unwrap_err().message.contains("startup"));
}

#[path = "const_eval_transport_tests.rs"]
mod compile_time_transport;

#[test]
fn constant_evaluation_checks_the_effects_of_local_class_drop_hooks() {
    let source = r#"def validate(n: int32):
    assert n > 0, "invalid guard"

class Guard:
    n: int32

    def __drop__(mut self):
        validate(self.n)

def read(n: int32) -> int32:
    guard = Guard(n)
    return guard.n

const answer = read(42)

def main():
    print(answer)
"#;
    assert!(check_source(source)
        .unwrap()
        .frozen_constants
        .contains_key("<main>::answer"));
    let error = check_source(&source.replace("read(42)", "read(0)")).unwrap_err();
    assert_eq!(error.code, "AU3026");
    assert!(error
        .call_frames
        .iter()
        .any(|f| f.function.contains("__drop__")));

    assert!(error.call_frames.iter().any(|f| f.function == "validate"));
    let step_limited = source.replace(
        "validate(self.n)",
        "mut i: int32 = 0\n        while i < 100000:\n            i += 1",
    );
    assert_eq!(check_source(&step_limited).unwrap_err().code, "AU3027");
    let memory_limited = source.replace("validate(self.n)", "text = \"x\".repeat(8388609)");
    assert_eq!(check_source(&memory_limited).unwrap_err().code, "AU3028");
    let effectful = source.replace("assert n > 0, \"invalid guard\"", "print(n)");
    assert_eq!(check_source(&effectful).unwrap_err().code, "AU3025");
    let ordinary = effectful.replace("const answer", "answer");
    assert!(!check_source(&ordinary)
        .unwrap()
        .frozen_constants
        .contains_key("<main>::answer"));
    assert_eq!(run_source(&ordinary).unwrap().stdout, "42\n42\n");
}

#[test]
fn values_with_a_live_drop_hook_keep_their_runtime_owner() {
    let source = r#"class Guard:
    n: int32

    def __drop__(mut self):
        assert self.n > 0, "invalid guard"

class Holder:
    guard: Guard

    def __drop__(mut self):
        assert false, "module owner must stay live"

values = [Holder(Guard(42))]
"#;
    assert!(!check_source(source)
        .unwrap()
        .frozen_constants
        .contains_key("<main>::values"));
    let required = source.replace("values =", "const values =");
    assert_eq!(check_source(&required).unwrap_err().code, "AU3025");
}

#[path = "const_eval_systems_tests.rs"]
mod systems_data;

#[test]
fn named_arguments_replace_defaults_during_purity_analysis() {
    let source = r#"def side() -> int64:
    print(5)
    return 5

def combine(base: int64, bonus: int64 = side()) -> int64:
    return base + bonus

const answer = combine(-7, bonus=4)
"#;
    assert_eq!(
        check_source(source).unwrap().frozen_constants["<main>::answer"],
        FrozenValue::Int(crate::integer::IntegerValue::from_i64(-3))
    );
    let omitted_default = source.replace("combine(-7, bonus=4)", "combine(-7)");
    assert_eq!(check_source(&omitted_default).unwrap_err().code, "AU3025");
}

#[test]
fn union_method_purity_checks_each_dispatch_arm() {
    let source = r#"trait Readable:
    def read(self) -> int64

class Quiet:
    n: int64

class Loud:
    n: int64

impl Readable for Quiet:
    def read(self) -> int64:
        return self.n

impl Readable for Loud:
    def read(self) -> int64:
        print(self.n)
        return self.n

def choose(item: Quiet | Loud) -> int64:
    return item.read()

const observed = choose(Quiet(13))
"#;
    assert_eq!(check_source(source).unwrap_err().code, "AU3025");
    let pure = source.replace("        print(self.n)\n", "");
    assert_eq!(
        check_source(&pure).unwrap().frozen_constants["<main>::observed"],
        FrozenValue::Int(crate::integer::IntegerValue::from_i64(13))
    );
}

#[test]
fn frozen_operations_preserve_absent_paths_and_set_operands() {
    let source = r#"import path
const extension = path.extension("README")

def combine() -> set[int64]:
    a: set[int64] = {1, 2}
    b: set[int64] = {2, 3}
    return a.union(b)

const combined = combine()
"#;
    let program = check_source(source).unwrap();
    assert!(matches!(
        &program.frozen_constants["<main>::extension"],
        FrozenValue::Union(_, _, inner) if **inner == FrozenValue::None
    ));
    assert!(matches!(
        &program.frozen_constants["<main>::combined"],
        FrozenValue::Set(_, elements) if elements.len() == 3
    ));
}

#[test]
fn serialized_float_operands_require_a_number_or_ieee_bits() {
    let mut input = serde_json::Deserializer::from_str(r#"{"bits":"NaN"}"#);
    let error = super::float_operand::deserialize(&mut input).unwrap_err();
    assert!(error
        .to_string()
        .contains("expected a float or its IEEE bits"));
}
