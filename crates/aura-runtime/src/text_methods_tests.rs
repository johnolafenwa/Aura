use super::{evaluate_bytes_search, evaluate_text_method, is_bytes_search_builtin};
use crate::runtime_value::Value;

#[test]
fn unknown_methods_fail_and_empty_text_repeats_to_empty() {
    let error = evaluate_text_method("shout", "x", None, 0).expect_err("no such method");
    assert!(error.message.contains("unknown text method `str.shout`"));
    let repeated = evaluate_text_method("repeat", "", None, 3).expect("repeat succeeds");
    assert_eq!(repeated, Value::string(""));
}

#[test]
fn byte_searches_answer_like_their_str_forms() {
    assert!(is_bytes_search_builtin("bytes::find"));
    assert!(!is_bytes_search_builtin("bytes::len"));
    assert_eq!(
        evaluate_bytes_search("bytes::starts_with", b"abc", b"ab"),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate_bytes_search("bytes::ends_with", b"abc", b"ab"),
        Value::Bool(false)
    );
}
