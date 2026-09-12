//! Bounded regression contracts for Batch 1 alias-cycle validation.

use aura_compiler::{check_source, Span};

struct CycleCase {
    source: &'static str,
    path: &'static str,
    closing_dependency: Span,
    declaration_spans: Vec<Span>,
}

fn assert_cycle(case: CycleCase) {
    let first = check_source(case.source).expect_err("a cyclic alias must be rejected");
    let second = check_source(case.source).expect_err("a cyclic alias must be rejected repeatedly");

    assert_eq!(
        first, second,
        "alias-cycle diagnostics must be deterministic"
    );
    assert_eq!(first.code, "AU2012");
    assert_eq!(first.message, format!("cyclic type alias: {}", case.path));
    assert_eq!(first.span, Some(case.closing_dependency));

    for declaration_span in &case.declaration_spans {
        assert!(
            first
                .secondary_spans
                .iter()
                .any(|secondary| secondary.span == *declaration_span),
            "every involved alias declaration must receive a secondary span: {first:?}"
        );
    }
    assert_eq!(
        first.secondary_spans.len(),
        case.declaration_spans.len(),
        "the diagnostic must label each involved declaration exactly once"
    );
}

#[test]
fn direct_alias_cycle_reports_the_closing_dependency() {
    assert_cycle(CycleCase {
        source: "type Loop = Loop\n",
        path: "Loop -> Loop",
        closing_dependency: Span::new(1, 13),
        declaration_spans: vec![Span::new(1, 1)],
    });
}

#[test]
fn indirect_alias_cycle_reports_every_involved_declaration() {
    assert_cycle(CycleCase {
        source: "type Left = Right\ntype Right = Left\n",
        path: "Left -> Right -> Left",
        closing_dependency: Span::new(2, 14),
        declaration_spans: vec![Span::new(1, 1), Span::new(2, 1)],
    });
}

#[test]
fn growing_generic_alias_cycle_is_rejected_without_expanding_it() {
    assert_cycle(CycleCase {
        source: "type Grow[T] = Grow[list[T]]\n",
        path: "Grow -> Grow",
        closing_dependency: Span::new(1, 16),
        declaration_spans: vec![Span::new(1, 1)],
    });
}

#[test]
fn nested_constructor_alias_cycle_is_still_detected() {
    assert_cycle(CycleCase {
        source: "type Outer = list[Inner]\ntype Inner = dict[str, Outer]\n",
        path: "Outer -> Inner -> Outer",
        closing_dependency: Span::new(2, 24),
        declaration_spans: vec![Span::new(1, 1), Span::new(2, 1)],
    });
}

#[test]
fn aliasing_a_valid_nominal_recursive_type_is_not_an_alias_cycle() {
    let source = "class Node:\n    next: indirect Node?\n\ntype NodeAlias = Node\n";
    check_source(source).expect("nominal indirection must remain a valid recursion boundary");
}

#[test]
fn specialized_alias_constructor_rejects_prospective_expansion_before_clone() {
    let mut argument = "int64".to_string();
    let mut template = "T".to_string();
    for _ in 0..70 {
        argument = format!("list[{argument}]");
        template = format!("list[{template}]");
    }
    let source = format!(
        "class Box[T]:\n    value: T\ntype Wrapped[T] = Box[{template}]\ndef main():\n    value = Wrapped[{argument}](value=[])\n"
    );

    let error = check_source(&source)
        .expect_err("constructor alias substitution must preflight its prospective depth");
    assert_eq!(error.code, "AU2999");
    assert_eq!(
        error.message,
        "type expansion exceeds recursion depth limit of 128"
    );
    assert_eq!(error.span, Some(Span::new(5, 13)));
}
