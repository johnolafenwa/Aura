use super::*;

#[test]
fn local_tables_restore_and_match_bounds_cover_the_public_regex_operations() {
    with_local_patterns(|| {
        crate::builtin_limit::with_allowance(1 << 20, || {
            let index = number("(?<word>a+)").unwrap();
            assert_eq!(number("(?<word>a+)").unwrap(), index);
            assert!(number("(").is_err());
            assert!(text(-1).is_err());
            with_local_patterns(|| {
                assert!(text(index).is_err());
                assert_eq!(number("b").unwrap(), 0);
            });
            let regex = compiled(index)?;
            assert_eq!(text(index)?.as_ref(), "(?<word>a+)");
            for name in [
                "find",
                "find_all",
                "matches",
                "split",
                "captures",
                "replace",
                "replace_all",
                "count",
            ] {
                bound_search(
                    &format!("regex::__aura_{name}"),
                    &regex,
                    "ab a",
                    &RegexArgument::Text("$word"),
                )?;
            }
            bound_search(
                "regex::__aura_replace",
                &regex,
                "ab a",
                &RegexArgument::None,
            )?;
            assert_eq!(
                crate::builtin_limit::with_allowance(4, || bound_search(
                    "regex::__aura_replace_all",
                    &regex,
                    "aaaaa",
                    &RegexArgument::Text("12345")
                ))
                .unwrap_err()
                .code,
                "AU3028"
            );
            Ok(())
        })
        .unwrap();
    });
    assert!(text(0).is_err());
    let exhausted = with_local_patterns(|| {
        crate::builtin_limit::with_allowance(64, || {
            assert!(number("[a-zA-Z]{100}").is_err());
            Ok(())
        })
    });
    assert_eq!(exhausted.unwrap_err().code, "AU3028");
}

#[test]
fn a_failed_initializer_releases_its_pattern_handles() {
    use crate::builtin_limit::with_allowance;

    let mut previous = -1;
    let exhausted = with_local_patterns(|| {
        with_allowance(1 << 20, || {
            previous = pattern_number("first").unwrap();
            with_allowance(8, || {
                // Compiling through the builtin reports a language-level error.
                // The evaluator must still see the exhausted table budget.
                host_regex_compile(vec![Value::string("second")], None)
            })
        })
    });
    assert_eq!(exhausted.unwrap_err().code, "AU3028");

    with_local_patterns(|| {
        with_allowance(1 << 20, || {
            let expired = evaluate_regex_builtin(
                "regex::__aura_is_match",
                previous,
                "first",
                RegexArgument::None,
            )
            .unwrap_err();
            assert!(expired.message.contains("invalid local regex pattern"));
            let current = pattern_number("second").unwrap();
            assert_eq!(current, previous);
            assert_eq!(pattern_text(current)?.as_ref(), "second");
            Ok(())
        })
    })
    .unwrap();
}

#[test]
fn captures_and_replacements_charge_the_text_they_duplicate() {
    use crate::builtin_limit::with_allowance;

    with_local_patterns(|| {
        with_allowance(1 << 20, || {
            let number = pattern_number("(?<word>a+)(b)?").unwrap();
            let subject = "a".repeat(32 * 1024);
            let captures = |subject: &str| {
                evaluate_regex_builtin(
                    "regex::__aura_captures",
                    number,
                    subject,
                    RegexArgument::None,
                )
            };
            // A missing optional group needs no copy of the subject. The
            // whole match and its named group each copy it once.
            assert!(with_allowance(48 * 1024, || captures("none")).is_ok());
            let Value::Tuple(result) = with_allowance(96 * 1024, || captures(&subject))? else {
                panic!("expected captures");
            };
            let Value::Vec(groups) = &result.elements[0] else {
                panic!("expected capture strings");
            };
            assert_eq!(groups.elements.len(), 3);
            assert_eq!(
                with_allowance(48 * 1024, || captures(&subject))
                    .unwrap_err()
                    .code,
                "AU3028"
            );

            let replace = |replacement| {
                evaluate_regex_builtin(
                    "regex::__aura_replace_all",
                    number,
                    &subject,
                    RegexArgument::Text(replacement),
                )
            };
            assert_eq!(
                with_allowance(72 * 1024, || replace("${word}"))?,
                Value::string(&subject)
            );
            assert_eq!(
                with_allowance(72 * 1024, || replace("${word}${word}${word}"))
                    .unwrap_err()
                    .code,
                "AU3028"
            );
            Ok(())
        })
    })
    .unwrap();
}
