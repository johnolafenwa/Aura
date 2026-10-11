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
