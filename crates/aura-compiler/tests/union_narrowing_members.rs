//! Field, method, and nested-place access through a union place that an
//! `is None` test narrowed to one class member.
use aura_compiler::{analyze_source, check_source, run_source};

fn rejects(source: &str, code: &str, expected: &str) {
    let error = check_source(source).expect_err(source);
    assert_eq!(error.code, code, "{}\n{source}", error.message);
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}` for:\n{source}",
        error.message
    );
}

fn runs(source: &str, expected: &str) {
    let output = run_source(source)
        .unwrap_or_else(|error| panic!("{source}\nfailed with {}: {}", error.code, error.message));
    assert_eq!(output.stdout, expected, "{source}");
}

const CLASSES: &str = "class Inner:\n    x: int64\n\nclass Point:\n    x: int64\n    inner: Inner\n    label: str\n\nclass Holder:\n    point: Point | None\n\ndef reset(point: mut Point | None):\n    point = None\n\ndef take_inner(inner: own Inner) -> int64:\n    return inner.x\n\n";

fn with_classes(body: &str) -> String {
    format!("{CLASSES}{body}")
}

#[test]
fn narrowed_object_reads_report_the_member_in_hover() {
    let analysis = analyze_source(&with_classes(
        "def main():\n    point: Point | None = Point(x=1, inner=Inner(x=2), label=\"a\")\n    if point is not None:\n        print(point.x)\n",
    ));
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let narrowed = analysis
        .occurrences
        .iter()
        .find(|occurrence| occurrence.line == 20 && occurrence.start_character == 14)
        .expect("the narrowed object read should be an occurrence");
    assert!(
        narrowed
            .hover
            .ends_with("Narrowed to `Point` from `Point | None` at this use."),
        "{}",
        narrowed.hover
    );
}

#[test]
fn a_mutable_union_argument_below_a_narrowed_object_ends_only_its_own_fact() {
    runs(
        &with_classes("def main():\n    mut holder: Holder | None = Holder(point=Point(x=1, inner=Inner(x=2), label=\"a\"))\n    if holder is not None and holder.point is not None:\n        reset(holder.point)\n        print(holder.point is None)\n"),
        "true\n",
    );
    rejects(
        &with_classes("def main():\n    mut holder: Holder | None = Holder(point=Point(x=1, inner=Inner(x=2), label=\"a\"))\n    if holder is not None and holder.point is not None:\n        reset(holder.point)\n        print(holder.point.x)\n"),
        "AU2014",
        "narrowing of `holder.point` no longer applies after a call with mutable access",
    );
}

#[test]
fn assigning_an_enclosing_place_ends_facts_below_it() {
    rejects(
        &with_classes("def main():\n    mut holder: Holder | None = Holder(point=Point(x=1, inner=Inner(x=2), label=\"a\"))\n    if holder is not None and holder.point is not None:\n        holder = None\n        print(holder.point.x)\n"),
        "AU2014",
        "narrowing of `holder` no longer applies after assignment",
    );
}

#[test]
fn moving_a_field_out_of_a_narrowed_payload_names_the_innermost_union() {
    rejects(
        &with_classes("def main():\n    holder: Holder | None = Holder(point=Point(x=1, inner=Inner(x=2), label=\"a\"))\n    if holder is not None and holder.point is not None:\n        print(take_inner(holder.point.inner))\n"),
        "AU3004",
        "cannot move non-copy field `inner` out of narrowed union `holder.point`",
    );
    let error = check_source(&with_classes("def main():\n    point: Point | None = Point(x=1, inner=Inner(x=2), label=\"a\")\n    if point is not None:\n        print(take_inner(point.inner))\n")).expect_err("a field move out of a narrowed payload");
    assert_eq!(error.code, "AU3004", "{}", error.message);
    let help = error.help.join("\n");
    assert!(
        help.contains("move the whole `point`") && !help.contains(".clone()"),
        "{help}"
    );
}

#[test]
fn whole_narrowed_unions_still_move_and_match_own_moves_fields() {
    runs(
        &with_classes("def keep(point: own Point) -> int64:\n    return point.x\n\ndef main():\n    point: Point | None = Point(x=4, inner=Inner(x=5), label=\"a\")\n    if point is not None:\n        print(point.x)\n        print(keep(point))\n    other: Point | None = Point(x=6, inner=Inner(x=7), label=\"b\")\n    match own other:\n        case Point(inner=inner):\n            print(take_inner(inner))\n        case None:\n            print(\"none\")\n"),
        "4\n4\n7\n",
    );
}
