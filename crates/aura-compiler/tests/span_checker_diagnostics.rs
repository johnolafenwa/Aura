//! Checker diagnostics for `Span[T]` members and slice-view sources.

use aura_compiler::check_source;

fn rejects(source: &str, code: &str, message: &str) {
    let error = check_source(source).expect_err("source should be rejected");
    assert_eq!(error.code, code, "{}", error.message);
    assert_eq!(error.message, message);
}

const SHARED: &str = "def main():\n    mut items = [1, 2]\n    view part = items[0:1]\n";
const MUTABLE: &str = "def main():\n    mut items = [1, 2]\n    view mut part = items[0:1]\n";

#[test]
fn span_member_arguments_are_checked() {
    rejects(
        &format!("{SHARED}    print(part.to_list(1))\n"),
        "AU2004",
        "`Span.to_list()` takes no arguments",
    );
    rejects(
        &format!("{MUTABLE}    part.fill()\n"),
        "AU2004",
        "`Span.fill(value)` takes one value argument",
    );
    rejects(
        &format!("{MUTABLE}    part.fill(other=1)\n"),
        "AU2004",
        "`Span.fill` has no parameter named `other`",
    );
    rejects(
        &format!("{MUTABLE}    part.fill(\"c\")\n"),
        "AU2002",
        "`fill` expects `int64`, found `str`",
    );
    rejects(
        &format!("{SHARED}    part.fill(1)\n"),
        "AU3003",
        "`fill` needs a mutable span; bind it with `view mut`",
    );
    rejects(
        "def main():\n    mut items = [\"a\", \"b\"]\n    view mut part = items[0:1]\n    part.fill(\"c\")\n",
        "AU3005",
        "`fill` requires a Copy element type, found `str`",
    );
}

#[test]
fn slice_views_need_a_list_source() {
    rejects(
        "def main():\n    text = \"abc\"\n    view part = text[0:1]\n    print(1)\n",
        "AU2003",
        "a slice view needs a `list[T]` or `Span[T]` source, found `str`",
    );
    rejects(
        "def f(text: str) -> view Span[str] from text:\n    return view text[0:1]\ndef main():\n    pass\n",
        "AU2003",
        "a returned slice view needs a `list[T]` or `Span[T]` source, found `str`",
    );
}

#[test]
fn span_types_are_refused_outside_view_positions() {
    rejects(
        "def f(p: Span[int64] = [1]) -> int64:\n    return len(p)\ndef main():\n    pass\n",
        "AU3010",
        "view type `Span[int64]` cannot be a parameter with a default",
    );
    rejects(
        "type R = Callable[def(p: Span[int64]) -> int64]\ndef main():\n    pass\n",
        "AU3010",
        "view type `Span[int64]` cannot appear in a function type",
    );
    rejects(
        "def f(p: Span[int64, str]) -> int64:\n    return 1\ndef main():\n    pass\n",
        "AU2002",
        "`Span` expects exactly one type argument",
    );
    rejects(
        "enum E:\n    A(Span[int64])\ndef main():\n    pass\n",
        "AU3010",
        "view type `Span[int64]` cannot be an enum payload type",
    );
    rejects(
        "class S:\n    x: int64\n\n    def m(self, items: list[int64]) -> Span[int64]:\n        return items[0:1]\n\ndef main():\n    pass\n",
        "AU3010",
        "view type `Span[int64]` cannot be an owned result type",
    );
    rejects(
        "trait K:\n    def m(self, items: list[int64]) -> Span[int64]\n\ndef main():\n    pass\n",
        "AU3010",
        "view type `Span[int64]` cannot be an owned result type",
    );
}

#[test]
fn a_span_argument_for_a_list_parameter_suggests_to_list() {
    let error = check_source(&format!(
        "def g(xs: list[int64]) -> int64:\n    return len(xs)\n{SHARED}    print(g(part))\n"
    ))
    .expect_err("a span is not an owned list");
    assert_eq!(error.code, "AU2002", "{}", error.message);
    assert_eq!(
        error.message,
        "argument type mismatch for function `g`: expected `list[int64]`, found `Span[int64]`"
    );
    assert!(
        error
            .help
            .iter()
            .any(|help| help.contains("span.to_list()")),
        "{:?}",
        error.help
    );
}

const RETURNS_SPAN: &str =
    "def head(items: list[int64]) -> view Span[int64] from items:\n    return view items[0:1]\n";

#[test]
fn a_returned_span_binds_with_its_own_kind_before_slicing() {
    rejects(
        &format!("{RETURNS_SPAN}def main():\n    items = [1, 2, 3]\n    view p = head(items)[0:1]\n    print(len(p))\n"),
        "AU3004",
        "bind the returned view with `view` before taking a slice view of it",
    );
    rejects(
        &format!("{RETURNS_SPAN}def main():\n    mut items = [1, 2, 3]\n    view mut p = head(items)\n    print(len(p))\n"),
        "AU3010",
        "a returned view must initialize a view binding with the same shared or mutable kind",
    );
}
