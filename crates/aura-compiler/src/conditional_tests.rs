use crate::{check_source, parse_source, run_source};

#[test]
fn contextual_suites_parse_and_inactive_names_are_not_resolved() {
    let source = include_str!("../tests/fixtures/parse-pass/conditional_declarations.au");
    assert!(parse_source(source).is_ok());
    let source = include_str!("../tests/fixtures/check-pass/conditional_inactive_names.au");
    check_source(source).unwrap();
}

#[test]
fn computed_predicate_selects_one_definition() {
    let source = include_str!("../tests/fixtures/run-pass/conditional_computed_predicate.au");
    assert_eq!(run_source(source).unwrap().stdout, "16\n");
}

#[test]
fn malformed_inactive_body_keeps_its_source_location() {
    let error = parse_source("import build\nif build.freestanding:\n    def broken(:\n        pass\n")
        .unwrap_err();
    assert_eq!(error.span.unwrap().line, 3);
}

#[test]
fn compile_time_condition_requires_a_boolean() {
    let source = include_str!("../tests/fixtures/check-fail/conditional_invalid_predicate.au");
    assert_eq!(check_source(source).unwrap_err().code, "AU2071");
}
