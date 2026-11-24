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
