//! Checker diagnostics for specialization, tuple indexing, and enum members.

use aura_compiler::check_source;

fn rejects(source: &str, expected: &str) {
    let error = check_source(source).expect_err("source should be rejected");
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}`",
        error.message
    );
}

#[test]
fn function_specialization_requires_type_arguments() {
    rejects(
        "def identity[T](value: own T) -> T:\n    return value\ndef main():\n    print(identity[1](2))\n",
        "function specialization expects type arguments",
    );
}

#[test]
fn tuple_index_out_of_range_is_rejected() {
    rejects(
        "def main():\n    pair = (1, 2)\n    print(pair[5])\n",
        "out of bounds",
    );
}

#[test]
fn unknown_enum_variants_are_rejected() {
    rejects(
        "enum Shape:\n    Circle(int64)\ndef main():\n    shape = Shape.Triangle\n    print(1)\n",
        "has no variant `Triangle`",
    );
}

#[test]
fn explicit_enum_type_arguments_reject_unknown_variants() {
    rejects(
        "enum Box[T]:\n    Full(T)\n    Empty\ndef main():\n    boxed = Box[int64].Missing\n    print(1)\n",
        "has no variant `Missing`",
    );
}

#[test]
fn unresolved_generic_results_name_the_type_parameter() {
    rejects(
        "def make[T]() -> list[T]:\n    return []\ndef main():\n    values = make()\n    print(values.len())\n",
        "T",
    );
}

#[test]
fn duration_literals_reject_separators_and_overflow() {
    let separators = aura_compiler::parse_source("def main():\n    print(1_000ms)\n")
        .expect_err("integer separators do not apply to duration literals");
    assert!(
        separators.message.contains("separators"),
        "{}",
        separators.message
    );
    let overflow =
        aura_compiler::parse_source("def main():\n    print(99999999999999999999999999999999m)\n")
            .expect_err("an overflowing duration literal is rejected");
    assert!(
        overflow.message.contains("duration") || overflow.message.contains("literal"),
        "{}",
        overflow.message
    );
}

#[test]
fn match_expressions_reject_type_arms_outside_the_union() {
    rejects(
        "def main():\n    value: int64 | str = 1\n    result = match value:\n        case float64 as number: 1\n        case int64 as number: number\n        case str as text: text.len()\n    print(result)\n",
        "member",
    );
}

#[test]
fn none_patterns_require_none_to_be_a_member() {
    rejects(
        "def main():\n    value: int64 | str = 1\n    match value:\n        case None:\n            print(0)\n        case int64 as number:\n            print(number)\n        case str as text:\n            print(text)\n",
        "not a direct member",
    );
}

#[test]
fn match_expression_arms_must_agree_on_their_result_type() {
    rejects(
        "def main():\n    value: int64 | str = 1\n    result = match value:\n        case int64 as number: number\n        case str as text: text\n    print(result)\n",
        "expects",
    );
}
