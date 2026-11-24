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
