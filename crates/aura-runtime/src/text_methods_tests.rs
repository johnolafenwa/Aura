use super::{chr, evaluate_bytes_search, evaluate_text_method, is_bytes_search_builtin, ord};
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

#[test]
fn characters_convert_between_strings_and_scalar_values() {
    let listed = evaluate_text_method("chars", "aé😀", None, 0).expect("chars succeeds");
    let Value::Vec(listed) = listed else {
        panic!("chars returns a list");
    };
    assert_eq!(
        listed.elements,
        vec![Value::string("a"), Value::string("é"), Value::string("😀")]
    );
    assert_eq!(ord("A").expect("one character"), 65);
    assert_eq!(ord("😀").expect("one character"), 128_512);
    for text in ["", "ab"] {
        let error = ord(text).expect_err("not one character");
        assert_eq!(error.code, "AU4003");
    }
    assert_eq!(chr(233).expect("a scalar value"), Value::string("é"));
    for code in [-1, 0xD800, 0xDFFF, 0x11_0000, i64::MAX] {
        let error = chr(code).expect_err("not a scalar value");
        assert!(error.message.contains(&code.to_string()));
    }
}
