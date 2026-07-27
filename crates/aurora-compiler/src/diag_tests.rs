use std::collections::BTreeSet;

use super::{Diagnostic, DiagnosticSeverity, Span, DIAGNOSTIC_CODE_REGISTRY};

#[test]
fn renders_annotated_diagnostics_with_source_context() {
    let diagnostic = Diagnostic::at(Span::new(2, 9), "unknown name `value`");
    let rendered =
        diagnostic.render_with_source("examples/demo.au", "def main():\n    print(value)\n");

    assert!(rendered.contains("error[AU2001]: unknown name `value`"));
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
        "error[AU2999]: plain failure\n --> examples/demo.au"
    );

    let out_of_range = Diagnostic::at(Span::new(4, 3), "missing line");
    let rendered = out_of_range.render_with_source("examples/demo.au", "def main():\n");
    assert!(rendered.contains("error[AU2999]: missing line"));
    assert!(rendered.contains("--> examples/demo.au:4:3"));
    assert_eq!(out_of_range.to_string(), "4:3: missing line");
}

#[test]
fn structured_diagnostics_preserve_codes_labels_help_and_edits() {
    let diagnostic = Diagnostic::coded_at("AU3001", Span::new(4, 11), "use of moved value `item`")
        .with_secondary(Span::new(2, 18), "value moved here")
        .with_note("non-copy values have one owner")
        .with_help("pass shared access or clone the value")
        .with_edit(Span::new(2, 11), Span::new(2, 11), ".clone()");

    let report = diagnostic.structured("examples/move.au");
    assert_eq!(report.code, "AU3001");
    assert_eq!(report.severity, DiagnosticSeverity::Error);
    assert_eq!(report.message, "use of moved value `item`");
    assert_eq!(report.primary_span.unwrap().path, "examples/move.au");
    assert_eq!(
        report.secondary_spans[0].label.as_deref(),
        Some("value moved here")
    );
    assert_eq!(report.notes, ["non-copy values have one owner"]);
    assert_eq!(report.help, ["pass shared access or clone the value"]);
    assert_eq!(report.edits[0].replacement, ".clone()");
    assert_eq!(report.edits[0].applicability, "machine-applicable");

    let json = serde_json::to_value(diagnostic.structured("examples/move.au"))
        .expect("structured diagnostic should serialize");
    assert_eq!(json["code"], "AU3001");
    assert_eq!(json["severity"], "error");
    assert_eq!(json["primary_span"]["start"]["line"], 4);
    assert_eq!(json["secondary_spans"][0]["label"], "value moved here");
    assert_eq!(json["edits"][0]["replacement"], ".clone()");

    let rendered = diagnostic.render_with_source(
        "examples/move.au",
        "def main():\n    take(item)\n    pass\n    print(item)\n",
    );
    assert!(rendered.contains("[AU3001]"));
    assert!(rendered.contains("value moved here"));
    assert!(rendered.contains("note: non-copy values have one owner"));
    assert!(rendered.contains("help: pass shared access or clone the value"));
    assert!(rendered.contains("fix: replace examples/move.au:2:11-2:11 with `.clone()`"));
}

#[test]
fn uncoded_constructors_assign_stable_phase_banded_codes() {
    assert_eq!(Diagnostic::new("unexpected character `@`").code, "AU1001");
    assert_eq!(
        Diagnostic::new("expected expression, found end of file").code,
        "AU1101"
    );
    assert_eq!(Diagnostic::new("unknown name `value`").code, "AU2001");
    assert_eq!(Diagnostic::new("use of moved value `value`").code, "AU3001");
    assert_eq!(
        Diagnostic::new("integer overflow in addition").code,
        "AU4002"
    );
    assert_eq!(
        Diagnostic::new("integer value `2147483648` does not fit in `int32`").code,
        "AU4002"
    );
    assert_eq!(
        Diagnostic::new("maximum call depth of 256 exceeded while calling `loop`").code,
        "AU4001"
    );
    assert_eq!(
        Diagnostic::new("`main` must return `int32` or `None` in the bootstrap runtime").code,
        "AU2999",
        "compile-time checks must not enter the runtime band merely because their message mentions the runtime"
    );
}

#[test]
fn diagnostic_code_registry_is_unique_banded_and_append_only_shaped() {
    let mut codes = BTreeSet::new();
    for entry in DIAGNOSTIC_CODE_REGISTRY {
        assert!(codes.insert(entry.code), "duplicate code {}", entry.code);
        assert_eq!(entry.code.len(), 6);
        assert!(entry.code.starts_with("AU"));
        assert!(entry.code[2..].chars().all(|ch| ch.is_ascii_digit()));
        assert!(matches!(
            entry.band,
            "lexical" | "parse" | "names/types" | "ownership" | "runtime"
        ));
        assert!(!entry.title.is_empty());
    }
}

#[test]
fn runtime_boundary_normalization_keeps_runtime_diagnostics_in_the_runtime_band() {
    let generic = Diagnostic::new("unsupported runtime operation").into_runtime_trap();
    assert_eq!(generic.code, "AU4001");

    let misleading = Diagnostic::new("unknown MIR place `temporary`").into_runtime_trap();
    assert_eq!(misleading.code, "AU4001");

    let precise = Diagnostic::coded_at(
        "AU4003",
        Span::new(3, 8),
        "map key `missing` was not present",
    )
    .into_runtime_trap();
    assert_eq!(precise.code, "AU4003");
    assert_eq!(precise.span, Some(Span::new(3, 8)));
}
