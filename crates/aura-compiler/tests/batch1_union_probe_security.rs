//! Bounded adversarial contracts for transactional union-literal probing.

use aura_compiler::{check_source, Span};

#[test]
fn numeric_range_mismatch_is_a_nonmatch_while_capacity_errors_still_escape() {
    let error = check_source("def main():\n    value: int8 | uint8 = 256\n")
        .expect_err("the literal fits neither union member");
    assert_eq!(error.code, "AU2010", "{error}");
    assert_eq!(error.message, "'int64' is not a member of 'int8 | uint8'");

    // The companion deep-expansion test below pins that a real capacity
    // diagnostic still escapes candidate probing unchanged.
}

#[test]
fn capacity_diagnostic_is_preserved_when_every_candidate_probe_hits_the_limit() {
    let mut argument = "int64".to_string();
    let mut template = "T".to_string();
    for _ in 0..70 {
        argument = format!("list[{argument}]");
        template = format!("list[{template}]");
    }
    let source = format!(
        "class Box[T]:\n    value: T\ntype Wrapped[T] = Box[{template}]\ndef main():\n    callback: (def() -> int64) | (def() -> str) = lambda: Wrapped[{argument}](value=[])\n"
    );

    let first = check_source(&source).expect_err("the probe must preserve its capacity error");
    let second = check_source(&source).expect_err("capacity diagnostics must be deterministic");
    assert_eq!(first, second);
    assert_eq!(first.code, "AU2999", "{first}");
    assert_eq!(
        first.message,
        "type expansion exceeds recursion depth limit of 128"
    );
    assert_eq!(first.span, Some(Span::new(5, 59)));
}

#[test]
fn contextual_literal_candidate_count_is_bounded_before_repeated_probing() {
    let mut source = String::new();
    for index in 0..129 {
        source.push_str(&format!("class C{index}:\n    pass\n"));
    }
    let members = (0..129)
        .map(|index| format!("list[C{index}]"))
        .collect::<Vec<_>>()
        .join(" | ");
    source.push_str(&format!("def main():\n    value: {members} = []\n"));

    let error = check_source(&source)
        .expect_err("an adversarial union must not trigger an unbounded candidate probe loop");
    assert_eq!(error.code, "AU2999", "{error}");
    assert!(
        error.message.contains("union") && error.message.contains("probe"),
        "{error}"
    );
}
