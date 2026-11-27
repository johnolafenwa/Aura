//! Inputs that once crashed or stalled the parser, the checker, MIR
//! lowering, or the interpreter on values nested deep. Each must finish
//! quickly with a result or a diagnostic.

use std::time::{Duration, Instant};

fn assert_fails_with(source: &str, code: &str, message_part: &str) {
    let error = crate::check_source(source).expect_err("the program is rejected");
    assert_eq!(error.code, code, "{error:?}");
    assert!(error.message.contains(message_part), "{error:?}");
}

// ----- B-MED-18: reserved names in an `extern "C"` union -----------------

#[test]
fn extern_union_with_reserved_names_reports_a_diagnostic() {
    for source in [
        "extern \"C\" union U:\n    __invariant__: int32\n",
        "extern \"C\" union view:\n    x: int32\n",
        "extern \"C\" union dyn:\n    x: int32\n",
    ] {
        let error = crate::parse_source(source).expect_err("the union is rejected");
        assert!(
            error
                .message
                .contains("cannot use these union or member names"),
            "{error:?}"
        );
        assert_eq!(error.span.map(|span| span.line), Some(1), "{error:?}");
        assert!(crate::format::format_source(source).is_err());
        let analysis = crate::analysis::analyze_source(source);
        assert!(!analysis.diagnostics.is_empty());
    }
}

// ----- B-MED-19: many items in one `with` header -------------------------

fn with_items(count: usize) -> String {
    let items = (0..count)
        .map(|index| format!("x{index} = 1"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("def main():\n    with {items}:\n        pass\n")
}

#[test]
fn with_items_count_against_the_recursion_limit() {
    let source = with_items(crate::limits::RECURSION_LIMIT + 1);
    let started = Instant::now();
    let error = crate::parse_source(&source).expect_err("too many items");
    assert_eq!(error.code, "AU1101", "{error:?}");
    assert!(error.message.contains("recursion limit"), "{error:?}");
    let source = with_items(1500);
    assert!(crate::parse_source(&source).is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
    // A header within the limit still parses.
    crate::parse_source(&with_items(8)).expect("eight items parse");
}

// ----- B-MED-20: an inferred type that grows by a power of the source ----

#[test]
fn nested_tuple_types_stay_within_the_node_limit() {
    let mut source = String::from("def main():\n    x0 = (1, 1)\n");
    for level in 1..=12 {
        let previous = format!("x{}", level - 1);
        source.push_str(&format!(
            "    x{level} = ({previous}, {previous}, {previous}, {previous})\n"
        ));
    }
    let started = Instant::now();
    assert_fails_with(&source, "AU2999", "node limit");
    assert!(started.elapsed() < Duration::from_secs(20));
}

// ----- B-MED-21: a match on a wide tuple of `bool` -----------------------

#[test]
fn match_on_wide_bool_tuple_checks_quickly() {
    let source = r#"
type B1 = (bool, bool)
type B2 = (B1, B1)
type B3 = (B2, B2)
type B4 = (B3, B3)
type B5 = (B4, B4)
type B6 = (B5, B5)
def f(x: B6) -> int64:
    match x:
        case ((((((true, _), _), _), _), _), _):
            return 1
        case ((((((false, _), _), _), _), _), _):
            return 0

def main():
    pass
"#;
    let started = Instant::now();
    crate::check_source(source).expect("the arms cover every value");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn match_on_wide_bool_tuple_still_reports_a_missing_case() {
    let source = r#"
type B1 = (bool, bool)
type B2 = (B1, B1)
type B3 = (B2, B2)
type B4 = (B3, B3)
type B5 = (B4, B4)
def f(x: B5) -> int64:
    match x:
        case (((((true, _), _), _), _), _):
            return 1
        case (((((false, true), _), _), _), _):
            return 0

def main():
    pass
"#;
    assert!(crate::check_source(source).is_err());
}

// ----- B-MED-29: a long chain of method calls ----------------------------

#[test]
fn chained_method_calls_lower_quickly() {
    let chain = ".wrapping_add(1)".repeat(40);
    let source =
        format!("def f(x: int64) -> int64:\n    return x{chain}\n\ndef main():\n    print(f(0))\n");
    let started = Instant::now();
    crate::lower_source_to_mir(&source).expect("the chain lowers");
    assert!(started.elapsed() < Duration::from_secs(10));
    let output = crate::run_source(&source).expect("the chain runs");
    assert_eq!(output.stdout, "40\n");
}

// ----- B-MED-24 to B-MED-27: a long chain of `indirect` variants --------

#[test]
fn deep_indirect_chains_run_in_a_task_and_a_generator() {
    let source = r#"
from iter import Generator

enum Chain:
    Nil
    Cons(int64, indirect Chain)

def build(n: int64) -> Chain:
    mut x = Chain.Nil
    mut i: int64 = 0
    while i < n:
        x = Chain.Cons(i, x)
        i += 1
    return x

def worker(depth: int64) -> bool:
    a = build(depth)
    b = build(depth)
    s = str(a)
    return a == b and len(s) > depth

def gen() -> Generator[int64]:
    x = build(30000)
    yield 1

def main():
    with g = TaskGroup():
        t = g.start(worker, 30000)
        match t.result():
            case TaskResult.Ready(v):
                print(v)
            case _:
                print("other")
    for v in gen():
        print(v)
"#;
    let output = crate::run_source(source).expect("the chains run");
    assert_eq!(output.stdout, "true\n1\n");
}
