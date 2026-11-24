    use std::path::Path;

    #[test]
    fn trait_delegation_diagnostics() {
        for (name, code) in [
            ("missing", "AU2060"),
            ("type", "AU2061"),
            ("duplicate", "AU2062"),
            ("view", "AU2063"),
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "tests/fixtures/check-fail/trait_delegation_{name}.au"
            ));
            let source = std::fs::read_to_string(path).unwrap();
            let error = crate::check_source(&source).expect_err(name);
            assert_eq!(error.code, code, "{error}");
        }
    }

    #[test]
    fn trait_delegation_calls_preserve_the_selected_contract() {
        for name in ["trait_delegation", "trait_delegation_identity", "trait_delegation_methods", "trait_delegation_specialized"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/run-pass/{name}.au"));
            let source = std::fs::read_to_string(&path).unwrap();
            let output = crate::run_source(&source).unwrap();
            assert_eq!(output.stdout, std::fs::read_to_string(path.with_extension("stdout")).unwrap());
        }
    }

    #[test]
    fn trait_delegation_requires_owned_field_declarations() {
        for declaration in ["view value: int64", "value: view int64", "value: view mut int64"] {
            let source = format!("class Box:\n    {declaration}\n");
            assert_eq!(crate::parse_source(&source).unwrap_err().code, "AU2063");
        }
        assert!(crate::parse_source("class Box:\n    view: int64\n").is_ok());
    }

    #[test]
    fn trait_delegation_imported_associated_types() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/run-pass/trait_delegation_assoc.au");
        let output = crate::run_path(&path).unwrap();
        assert_eq!(output.stdout, "7\n");
    }
