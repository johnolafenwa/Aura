
use super::{Diagnostic, Span};

#[test]
fn renders_annotated_diagnostics_with_source_context() {
    let diagnostic = Diagnostic::at(Span::new(2, 9), "unknown name `value`");
    let rendered =
        diagnostic.render_with_source("examples/demo.au", "def main():\n    print(value)\n");

    assert!(rendered.contains("error: unknown name `value`"));
    assert!(rendered.contains("--> examples/demo.au:2:9"));
    assert!(rendered.contains("2 |     print(value)"));
    assert!(rendered.contains("|         ^"));
}

#[test]
fn renders_unspanned_and_out_of_range_diagnostics() {
    let plain = Diagnostic::new("plain failure");
    assert_eq!(plain.to_string(), "plain failure");
    assert_eq!(
        plain.render_with_source("examples/demo.au", "def main():\n    pass\n"),
        "error: plain failure\n --> examples/demo.au"
    );

    let out_of_range = Diagnostic::at(Span::new(4, 3), "missing line");
    let rendered = out_of_range.render_with_source("examples/demo.au", "def main():\n");
    assert!(rendered.contains("error: missing line"));
    assert!(rendered.contains("--> examples/demo.au:4:3"));
    assert_eq!(out_of_range.to_string(), "4:3: missing line");
}
