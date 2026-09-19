use std::{fs, path::PathBuf};

#[test]
fn complete_contract_differences_are_rejected_at_each_ordinary_destination() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check-fail");
    for difference in ["name", "default", "keyword", "kind", "obligation"] {
        for destination in [
            "annotation",
            "alias",
            "parameter",
            "field",
            "element",
            "return",
            "join",
        ] {
            let stem = format!("callable_exact_{difference}_{destination}");
            let source = fs::read_to_string(root.join(format!("{stem}.au"))).unwrap();
            let error = aura_compiler::check_source(&source)
                .err()
                .unwrap_or_else(|| panic!("{stem} must reject the differing bare contract"));
            assert_eq!(error.code, "AU2015", "{stem}: {error}");
            assert!(error.message.contains(difference), "{stem}: {error}");
            assert!(
                error.message.contains("callable contract mismatch"),
                "{stem}: {error}"
            );
        }
    }
}
