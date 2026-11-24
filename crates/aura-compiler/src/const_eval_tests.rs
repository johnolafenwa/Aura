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
    assert_eq!(run_source(source).unwrap().stdout, include_str!("../tests/fixtures/run-pass/frozen_module_values.stdout"));
}

#[test]
fn required_initializers_explain_impurity_and_keep_ordinary_effects_at_runtime() {
    let source = include_str!("../tests/fixtures/check-fail/required_effectful_constant.au");
    let error = check_source(source).unwrap_err();
    assert_eq!(error.code, "AU3025");
    assert_eq!(run_source(&source.replace("const token", "token")).unwrap().stdout, "effect\n");
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
