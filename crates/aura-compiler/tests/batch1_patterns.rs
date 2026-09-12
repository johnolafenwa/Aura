//! Ratified type-pattern contracts, installed before implementation.
use aura_compiler::{check_source, run_path_with_source};
use std::path::Path;

#[test]
fn partial_payload() {
    run_fixture("partial_payload");
}

#[test]
fn or_guard_once() {
    run_fixture("or_guard_once");
}

#[test]
fn nested_mutable() {
    run_fixture("nested_mutable");
}

#[test]
fn nested_root_retag() {
    rejects("nested_root_retag", "AU3002");
}

#[test]
fn nested_missing() {
    rejects("nested_missing", "AU2013");
}

#[test]
fn or_duplicate() {
    rejects("or_duplicate", "AU2013");
}

#[test]
fn nested_mutable_none() {
    run_fixture("nested_mutable_none");
}

#[test]
fn nested_mutable_return() {
    run_fixture("nested_mutable_return");
}

fn run_fixture(name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/run-pass")
        .join(format!("union_match_{name}.au"));
    let source = std::fs::read_to_string(&path).unwrap();
    let expected = std::fs::read_to_string(path.with_extension("stdout")).unwrap();
    let result =
        run_path_with_source(&path, &source).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(result.stdout, expected, "{name}");
}

macro_rules! runs {
    ($($name:ident),+ $(,)?) => { $(#[test] fn $name() { run_fixture(stringify!($name)); })+ };
}
runs!(
    nested_owned_guard,
    nested_owned_siblings,
    nested_mutable_break,
    nested_mutable_continue,
    nested_mutable_try,
    nested_generic_enum,
    nested_imported_enum,
    nested_option,
    nested_enum_chain,
    nested_union_chain,
    alias,
    expression,
    guard_once,
    integer,
    member_literal_nested,
    mutable_break,
    mutable_continue,
    mutable_payload,
    mutable_retag_after_arm,
    mutable_return,
    mutable_string,
    nested_enum,
    nested_or,
    none,
    owned_guard,
    owned_list,
    scrutinee_once,
    singleton,
    string,
    unit_singleton
);

fn rejects(name: &str, code: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check-fail")
        .join(format!("union_match_{name}.au"));
    let source = std::fs::read_to_string(path).unwrap();
    let error = check_source(&source).expect_err("ratified pattern restriction");
    assert_eq!(error.code, code, "{name}: {error}");
    assert!(
        error.span.is_some(),
        "pattern diagnostics must identify the source"
    );
}

macro_rules! rejects {
    ($($name:ident => $code:literal),+ $(,)?) => { $(#[test] fn $name() { rejects(stringify!($name), $code); })+ };
}
rejects!(missing => "AU2013", duplicate => "AU2013", nonmember => "AU2013",
    union_alias => "AU2013", literal => "AU2013", or_binding => "AU2013",
    guard_only => "AU2013", unreachable => "AU2013", retag_mutable => "AU3002",
    retag_shared_copy => "AU3002", payload_wrong_type => "AU2002",
    owned_root_reuse => "AU3001", shared_move => "AU3002");
