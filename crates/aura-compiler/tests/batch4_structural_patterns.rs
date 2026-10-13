//! Rest elements, list sequence patterns, and keyword class patterns:
//! checker diagnostics, coverage, and MIR execution edges.
use aura_compiler::{check_source, run_source};

fn rejects(source: &str, code: &str, expected: &str) {
    let error = check_source(source).expect_err(source);
    assert_eq!(error.code, code, "{}\n{source}", error.message);
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}` for:\n{source}",
        error.message
    );
}

fn accepts(source: &str) {
    if let Err(error) = check_source(source) {
        panic!("{source}\nrejected with {}: {}", error.code, error.message);
    }
}

fn runs(source: &str, expected: &str) {
    let output = run_source(source)
        .unwrap_or_else(|error| panic!("{source}\nfailed with {}: {}", error.code, error.message));
    assert_eq!(output.stdout, expected, "{source}");
}

const CLASSES: &str = "class Point:\n    x: int64\n    y: int64\n\n    def norm(self) -> int64:\n        return self.x + self.y\n\nclass Flag:\n    on: bool\n    label: str\n\nclass Box[T]:\n    value: T\n\nenum Shade:\n    Light\n    Dark\n\nclass Tile:\n    shade: Shade\n    size: int64\n\n";

fn with_classes(body: &str) -> String {
    format!("{CLASSES}{body}")
}

#[test]
fn nested_sequence_patterns_reject_non_list_elements_and_shadowing() {
    rejects(
        "def main():\n    match (1, 2):\n        case ([a], _):\n            print(a)\n        case _:\n            pass\n",
        "AU2999",
        "sequence pattern requires a `list[T]` scrutinee, found `int64`",
    );
    rejects(
        "def f(values: list[int64], rest: int64):\n    match values:\n        case [*rest]:\n            print(rest)\n\ndef main():\n    f([1], 2)\n",
        "AU2999",
        "would shadow an existing name",
    );
    rejects(
        "def main():\n    pair = ([1, 2], 3)\n    match own pair:\n        case ([first, *_], _):\n            print(first)\n        case _:\n            pass\n",
        "AU2041",
        "`match own` does not support sequence patterns",
    );
}

#[test]
fn nested_class_patterns_reject_wrong_types_and_mutable_matches() {
    rejects(
        &with_classes("def main():\n    match (1,):\n        case (Point(x=0),):\n            pass\n        case _:\n            pass\n"),
        "AU2040",
        "does not match scrutinee type `int64`",
    );
    rejects(
        &with_classes("def main():\n    mut value: Point | None = Point(x=1, y=2)\n    match mut value:\n        case Point(x=x):\n            print(x)\n        case None:\n            pass\n"),
        "AU2040",
        "`match mut` does not support class patterns",
    );
    rejects(
        &with_classes("def main():\n    match Point(x=1, y=2):\n        case Missing(x=0):\n            pass\n        case _:\n            pass\n"),
        "AU2040",
        "`Missing` is not a class",
    );
    rejects(
        &with_classes("def main():\n    match Point(x=1, y=2):\n        case Point(norm=0):\n            pass\n        case _:\n            pass\n"),
        "AU2040",
        "is a method of `Point`",
    );
}

#[test]
fn tuple_rest_patterns_report_nested_arity_failures() {
    rejects(
        "def main():\n    match ((1, 2), 3):\n        case ((a, b, c, *_), _):\n            print(a)\n",
        "AU2041",
        "tuple pattern has 3 elements besides its rest element but the scrutinee has only 2 positions",
    );
    rejects(
        "def main():\n    match ((1, 2), 3):\n        case ((a, b, *_, c), _):\n            print(a)\n",
        "AU2041",
        "tuple pattern has 3 elements besides its rest element",
    );
    rejects(
        "def main():\n    match ((1, 2), 3):\n        case ((a, *more, b), _):\n            print(a)\n",
        "AU2041",
        "covers no positions",
    );
    rejects(
        "def main():\n    match ((1, 2), 3):\n        case ((a, b, c), _):\n            print(a)\n",
        "AU2999",
        "tuple pattern has 3 elements but the scrutinee has 2",
    );
    rejects(
        "def main():\n    a, (b, *c) = (1, (2,))\n    print(a)\n",
        "AU2041",
        "covers no positions",
    );
}

#[test]
fn sequence_coverage_tracks_lengths_or_patterns_and_reachability() {
    accepts("def f(values: list[int64]) -> int64:\n    return match values:\n        case [] | [_]: 0\n        case [_, _, *_]: 1\n\ndef main():\n    print(f([]))\n");
    accepts("def f(values: list[int64]) -> int64:\n    return match values:\n        case []: 0\n        case [a]: a\n        case [a, b]: a + b\n        case [a, *_, b, c]: a + b + c\n\ndef main():\n    print(f([]))\n");
    rejects(
        "def f(values: list[int64]) -> int64:\n    return match values:\n        case []: 0\n        case [a, b, *_]: a + b\n\ndef main():\n    print(f([]))\n",
        "AU2999",
        "non-exhaustive match over `list[int64]`",
    );
    rejects(
        "def f(values: list[int64]) -> int64:\n    return match values:\n        case [1, *_]: 0\n        case []: 1\n\ndef main():\n    print(f([]))\n",
        "AU2999",
        "non-exhaustive match over `list[int64]`",
    );
    for (first, second) in [
        ("[_, *_]", "[a, b]"),
        ("[a]", "[b]"),
        ("[*_, x]", "[*_, 1, y]"),
        ("[x, *_]", "[1, *rest]"),
        ("[x, *_, y]", "[1, 2, 3]"),
    ] {
        rejects(
            &format!("def f(values: list[int64]) -> int64:\n    return match values:\n        case {first}: 0\n        case {second}: 1\n        case _: 2\n\ndef main():\n    print(f([]))\n"),
            "AU2999",
            "unreachable",
        );
    }
    for (first, second) in [
        ("[a, b]", "[a]"),
        ("[1, *_]", "[2, *_]"),
        ("[*_, 1]", "[2]"),
        ("[a, b, *_]", "[c]"),
        ("[a]", "[b, *_]"),
    ] {
        accepts(&format!("def f(values: list[int64]) -> int64:\n    return match values:\n        case {first}: 0\n        case {second}: 1\n        case _: 2\n\ndef main():\n    print(f([]))\n"));
    }
}

#[test]
fn class_coverage_uses_field_columns_or_patterns_and_union_members() {
    accepts(&with_classes("def f(flag: Flag) -> int64:\n    return match flag:\n        case Flag(on=true) | Flag(on=false): 1\n\ndef main():\n    print(f(Flag(on=true, label=\"a\")))\n"));
    accepts(&with_classes("def f(tile: Tile) -> int64:\n    return match tile:\n        case Tile(shade=Shade.Light): 1\n        case Tile(shade=Shade.Dark, size=size): size\n\ndef main():\n    print(f(Tile(shade=Shade.Dark, size=3)))\n"));
    accepts(&with_classes("def f(value: Point | Flag) -> int64:\n    return match value:\n        case Point(): 1\n        case Flag(): 2\n\ndef main():\n    print(f(Point(x=1, y=2)))\n"));
    accepts(&with_classes("def f(pair: (Point, bool)) -> int64:\n    return match pair:\n        case (Point(x=x), true): x\n        case (Point(), false): 0\n\ndef main():\n    print(f((Point(x=1, y=2), true)))\n"));
    rejects(
        &with_classes("def f(value: Point | Flag) -> int64:\n    return match value:\n        case Point(): 1\n        case Point(x=1): 2\n        case Flag(): 3\n\ndef main():\n    print(f(Point(x=1, y=2)))\n"),
        "AU2013",
        "unreachable type arm",
    );
    rejects(
        &with_classes("def f(tile: Tile) -> int64:\n    return match tile:\n        case Tile(shade=Shade.Light): 1\n\ndef main():\n    print(f(Tile(shade=Shade.Dark, size=3)))\n"),
        "AU2999",
        "non-exhaustive match over `Tile`",
    );
    rejects(
        &with_classes("def f(value: Point | Flag) -> int64:\n    return match value:\n        case Point(x=0): 1\n        case Flag(): 2\n\ndef main():\n    print(f(Point(x=1, y=2)))\n"),
        "AU2013",
        "",
    );
}

#[test]
fn structural_patterns_run_through_rest_slices_and_owned_classes() {
    runs(
        "def main():\n    values = [\"a\", \"b\", \"c\", \"d\", \"e\"]\n    match values:\n        case [first, *middle, before, last]:\n            print(first)\n            print(middle)\n            print(before)\n            print(last)\n        case _:\n            print(\"short\")\n    match [1]:\n        case [only, *rest]:\n            print(only)\n            print(len(rest))\n        case _:\n            pass\n",
        "a\n[b, c]\nd\ne\n1\n0\n",
    );
    runs(
        &with_classes("def main():\n    boxed = Box(value=[Point(x=1, y=2), Point(x=3, y=4)])\n    match boxed:\n        case Box(value=[Point(x=a), *others]):\n            print(a)\n            print(len(others))\n        case _:\n            print(\"other\")\n    values: Flag | Point | None = Flag(on=true, label=\"lamp\")\n    match own values:\n        case Point(x=x) if x > 0:\n            print(x)\n        case Flag(on=true, label=label):\n            print(label)\n        case _:\n            print(\"none\")\n"),
        "1\n1\nlamp\n",
    );
    runs(
        "def main():\n    for first, *rest in [(1, \"a\", true), (2, \"b\", false)]:\n        print(first)\n        print(rest)\n    pair = (\"x\", \"y\", \"z\")\n    match own pair:\n        case (head, *tail):\n            print(head)\n            print(tail)\n    *_, last = (1, 2, 3)\n    print(last)\n",
        "1\n(a, true)\n2\n(b, false)\nx\n(y, z)\n3\n",
    );
}

#[test]
fn structural_pattern_bindings_reject_shadowing_in_every_position() {
    rejects(
        "def f(rest: int64):\n    match (1, 2, 3):\n        case (a, *rest):\n            print(a)\n\ndef main():\n    f(1)\n",
        "AU2999",
        "shadow",
    );
    rejects(
        "def f(x: int64, values: list[int64]):\n    match values:\n        case [x]:\n            print(x)\n        case _:\n            pass\n\ndef main():\n    f(1, [2])\n",
        "AU2999",
        "shadow",
    );
    rejects(
        &with_classes("def f(x: int64, point: Point):\n    match point:\n        case Point(x=x):\n            print(x)\n        case _:\n            pass\n\ndef main():\n    f(1, Point(x=1, y=2))\n"),
        "AU2999",
        "shadow",
    );
    rejects(
        "def main():\n    a, *a = (1, 2)\n    print(a)\n",
        "AU1101",
        "duplicate binding target `a`",
    );
}

#[test]
fn structural_patterns_parse_errors_name_the_fix() {
    for (source, code, expected) in [
        (
            "def main():\n    match (1, 2):\n        case (*1, a):\n            pass\n",
            "AU1101",
            "a rest pattern is `*name` or `*_`",
        ),
        (
            "def main():\n    match Ok(1):\n        case Ok(a, value=1):\n            pass\n        case _:\n            pass\n",
            "AU2040",
            "cannot mix positional and keyword subpatterns",
        ),
    ] {
        let error = aura_compiler::parse_source(source).expect_err(source);
        assert_eq!(error.code, code, "{}", error.message);
        assert!(error.message.contains(expected), "{}", error.message);
    }
}

#[test]
fn structural_patterns_over_the_wrong_root_scrutinee_are_rejected() {
    rejects(
        &with_classes("def f(shade: Shade) -> int64:\n    return match shade:\n        case Point(x=0): 1\n        case _: 0\n\ndef main():\n    print(f(Shade.Light))\n"),
        "AU2040",
        "does not match scrutinee type",
    );
    rejects(
        "def f(shade: Result[int64, str]) -> int64:\n    return match shade:\n        case [a]: a\n        case _: 0\n\ndef main():\n    print(f(Ok(1)))\n",
        "AU2999",
        "sequence pattern requires a `list[T]` scrutinee",
    );
    rejects(
        &with_classes("def f(shade: Shade):\n    match shade:\n        case Point(x=0):\n            pass\n        case _:\n            pass\n\ndef main():\n    f(Shade.Light)\n"),
        "AU2040",
        "does not match scrutinee type",
    );
    rejects(
        &with_classes("def f(pair: (int64, int64)) -> int64:\n    return match pair:\n        case Point(x=0): 1\n        case _: 0\n\ndef main():\n    print(f((1, 2)))\n"),
        "AU2040",
        "",
    );
}

#[test]
fn or_patterns_compare_structural_bindings() {
    accepts(&with_classes("def f(values: list[int64]) -> int64:\n    return match values:\n        case [a] | [_, a]: a\n        case _: 0\n\ndef main():\n    print(f([1]))\n"));
    accepts(&with_classes("def f(value: Point | Box[int64]) -> int64:\n    return match value:\n        case Point(x=a) | Box(value=a): a\n\ndef main():\n    print(f(Point(x=1, y=2)))\n"));
    accepts("def f(triple: (int64, int64, int64)) -> int64:\n    return match triple:\n        case (0, *rest) | (*rest, 0): rest[0]\n        case _: 0\n\ndef main():\n    print(f((0, 1, 2)))\n");
    rejects(
        "def f(values: list[int64]) -> int64:\n    return match values:\n        case [a] | [_, b]: 1\n        case _: 0\n\ndef main():\n    print(f([1]))\n",
        "AU2999",
        "",
    );
}

#[test]
fn guarded_and_binding_arms_mix_with_structural_coverage() {
    accepts("def f(values: list[int64]) -> int64:\n    return match values:\n        case whole if len(whole) > 3: 9\n        case []: 0\n        case [_, *_]: 1\n\ndef main():\n    print(f([]))\n");
    accepts(&with_classes("def f(point: Point) -> int64:\n    return match point:\n        case whole if whole.x > 3: 9\n        case Point(x=0, y=x) | Point(x=x): x\n\ndef main():\n    print(f(Point(x=1, y=2)))\n"));
}

#[test]
fn structural_patterns_run_over_temporaries_payloads_and_comprehensions() {
    runs(
        &with_classes("def make() -> Point:\n    return Point(x=5, y=6)\n\ndef triple() -> (str, str, str):\n    return (\"p\", \"q\", \"r\")\n\ndef main():\n    match make():\n        case Point(x=x, y=y):\n            print(x + y)\n    match own triple():\n        case (first, *rest):\n            print(first)\n            print(rest)\n    result: Result[(str, str, str), str] = Ok((\"a\", \"b\", \"c\"))\n    match own result:\n        case Ok((head, *tail)):\n            print(head)\n            print(tail)\n        case Err(message):\n            print(message)\n    holder: Result[Flag, str] = Ok(Flag(on=true, label=\"lit\"))\n    match own holder:\n        case Ok(Flag(label=label)):\n            print(label)\n        case Err(message):\n            print(message)\n    firsts = [a for a, *_ in [(1, 2, 3), (4, 5, 6)]]\n    print(firsts)\n    lasts = [rest for _, *rest in [(1, 2, 3)]]\n    print(lasts)\n"),
        "11\np\n(q, r)\na\n(b, c)\nlit\n[1, 4]\n[(2, 3)]\n",
    );
}

#[test]
fn analysis_resolves_class_patterns_through_module_aliases() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/run-pass/class_pattern_imported.au");
    let source = std::fs::read_to_string(&path).expect("fixture source");
    let analysis = aura_compiler::analyze_path_source(&path, &source);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(analysis
        .occurrences
        .iter()
        .any(|occurrence| occurrence.hover.contains("x: int64") && occurrence.line == 5));
}

#[test]
fn sequence_patterns_run_over_temporary_lists() {
    runs(
        "def make() -> list[str]:\n    return [\"x\", \"y\", \"z\"]\n\ndef main():\n    match make():\n        case [first, *rest]:\n            print(first)\n            print(rest)\n        case []:\n            print(\"empty\")\n    match [make(), make()]:\n        case [[a, *_], [*_, b]]:\n            print(a + b)\n        case _:\n            print(\"other\")\n",
        "x\n[y, z]\nxz\n",
    );
}

#[test]
fn structural_patterns_run_inside_lambdas_and_guarded_owned_matches() {
    runs(
        "class Flag:\n    on: bool\n    label: str\n\ndef main():\n    base = 10\n    numbers = [1, 2, 3]\n    flag = Flag(on=true, label=\"abc\")\n    triple = (1, 2, 3)\n    pick: def(int64) -> int64 = lambda extra: match numbers:\n        case [first, *rest]:\n            first + base + len(rest) + extra\n        case _:\n            base\n    print(pick(0))\n    flagger: def(int64) -> int64 = lambda extra: match flag:\n        case Flag(on=true, label=label):\n            len(label) + base + extra\n        case _:\n            0\n    print(flagger(0))\n    tupler: def(int64) -> int64 = lambda extra: match triple:\n        case (a, *rest):\n            a + base + extra\n    print(tupler(0))\n    result: Result[(str, str, str), str] = Ok((\"a\", \"b\", \"c\"))\n    match own result:\n        case Ok((head, *tail)) if head == \"a\":\n            print(tail)\n        case _:\n            print(\"other\")\n    holder: Result[Flag, str] = Ok(Flag(on=true, label=\"lit\"))\n    match own holder:\n        case Ok(Flag(label=label)) if label != \"\":\n            print(label)\n        case _:\n            print(\"none\")\n",
        "13\n13\n11\n(b, c)\nlit\n",
    );
}

#[test]
fn refutable_class_arms_over_unions_track_reachability() {
    let arms = |body: &str| {
        with_classes(&format!(
            "def f(value: Point | Flag) -> int64:\n    return match value:\n{body}        case _: 0\n\ndef main():\n    print(f(Point(x=1, y=2)))\n"
        ))
    };
    rejects(
        &arms("        case Point(x=1, y=_): 1\n        case Point(x=1, y=2): 2\n"),
        "AU2013",
        "unreachable type arm",
    );
    rejects(
        &arms("        case Point(x=1) | Flag(on=true): 1\n        case Point(x=1, y=2): 2\n"),
        "AU2013",
        "unreachable type arm",
    );
    accepts(&arms(
        "        case Point(x=1): 1\n        case Point(x=2): 2\n        case Flag(on=true): 3\n",
    ));
    accepts(&arms(
        "        case Flag(on=true): 1\n        case Point(x=2): 2\n",
    ));
}

#[test]
fn owned_matches_move_nested_rest_tuples_and_class_fields() {
    runs("class Flag:\n    on: bool\n    label: str\n\ndef make() -> Flag:\n    return Flag(on=true, label=\"made\")\n\ndef main():\n    nested = ((\"a\", \"b\", \"c\"), \"d\")\n    match own nested:\n        case ((x, *r), d):\n            print(x)\n            print(r)\n            print(d)\n    match own make():\n        case Flag(label=label):\n            print(label)\n    either: Result[Flag, Flag] = Err(Flag(on=false, label=\"bad\"))\n    match own either:\n        case Ok(Flag(label=label)) | Err(Flag(label=label)):\n            print(label)\n", "a\n(b, c)\nd\nmade\nbad\n");
}

#[test]
fn rest_targets_reject_shadowing_names() {
    rejects(
        "def f(rest: int64):\n    for a, *rest in [(1, 2, 3)]:\n        print(a)\n\ndef main():\n    f(1)\n",
        "AU2999",
        "loop binding `rest` would shadow an existing name",
    );
    rejects(
        "def main():\n    rest = 1\n    a, *rest = (1, 2, 3)\n    print(rest)\n",
        "AU2999",
        "tuple binding `rest` would shadow an existing name",
    );
}
