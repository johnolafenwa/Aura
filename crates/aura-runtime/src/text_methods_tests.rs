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

#[test]
fn char_at_counts_characters_from_either_end() {
    for (text, ascii) in [("héllo", false), ("hello", true), ("hello", false)] {
        let first = super::char_at(text, ascii, 0).expect("index 0");
        assert_eq!(first, Value::string("h"));
        let last = super::char_at(text, ascii, -1).expect("index -1");
        assert_eq!(last, Value::string("o"));
        let second = super::char_at(text, ascii, -4).expect("index -4");
        assert_eq!(
            second,
            Value::string(text.chars().nth(1).unwrap().to_string())
        );
        for index in [5, -6, i64::MAX, i64::MIN] {
            let error = super::char_at(text, ascii, index).expect_err("out of range");
            assert_eq!(error.code, "AU4003");
            assert_eq!(
                error.message,
                format!("str index {index} is out of range for a string of 5 characters")
            );
        }
    }
    assert_eq!(
        evaluate_text_method("char_at", "日本語", None, 1).expect("char_at succeeds"),
        Value::string("本")
    );
    let empty = super::char_at("", true, 0).expect_err("empty text has no character");
    assert!(empty.message.contains("a string of 0 characters"));
}
