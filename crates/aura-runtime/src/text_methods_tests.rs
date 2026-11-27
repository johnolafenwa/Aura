use super::{
    change_case_checked, chars, chr, evaluate_bytes_search, evaluate_text_method,
    is_bytes_search_builtin, join_checked, ord, replace_checked,
};
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
fn indexed_characters_use_inline_text_storage() {
    for (source, ascii, index, expected) in [("abc", true, -1, "c"), ("a😀z", false, 1, "😀")] {
        let Value::String(character) = super::char_at(source, ascii, index).unwrap() else {
            panic!("indexing a string yields a string");
        };
        assert_eq!(character.as_str(), expected);
        assert_eq!(character.capacity(), crate::text::INLINE_CAPACITY);
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

#[test]
fn join_replace_and_case_changes_stop_at_the_string_limit() {
    use crate::runtime_value::MAX_STRING_BYTES;
    assert_eq!(join_checked(&["a", "b", "c"], ", ").unwrap(), "a, b, c");
    assert_eq!(join_checked::<&str>(&[], ", ").unwrap(), "");
    // 1000 parts joined by a 70000-byte separator is about 70 MB.
    let separator = "s".repeat(70_000);
    let parts = vec!["x"; 1000];
    assert_eq!(join_checked(&parts, &separator).unwrap_err().code, "AU4005");

    for (text, from, to) in [
        ("a.b.c", ".", "--"),
        ("abc", "", "-"),
        ("aaaa", "aa", "b"),
        ("été", "t", "TT"),
        ("none", "x", "y"),
        ("", "", "z"),
    ] {
        assert_eq!(
            replace_checked(text, from, to).unwrap(),
            text.replace(from, to)
        );
    }
    let wide = "a".repeat(100_000);
    let long = "b".repeat(1_000);
    assert_eq!(
        replace_checked(&wide, "a", &long).unwrap_err().code,
        "AU4005"
    );
    assert_eq!(
        replace_checked(&wide, "", &long).unwrap_err().code,
        "AU4005"
    );
    // The fast bound is not a trap: few real matches still fit.
    let sparse = format!("{}x", "a".repeat(MAX_STRING_BYTES / 2));
    assert_eq!(
        replace_checked(&sparse, "x", "yz").unwrap().len(),
        sparse.len() + 1
    );

    assert_eq!(change_case_checked("Straße", true).unwrap(), "STRASSE");
    assert_eq!(change_case_checked("ΣΑΣ", false).unwrap(), "σας");
    assert_eq!(change_case_checked("MiXed", false).unwrap(), "mixed");
    assert_eq!(change_case_checked("MiXed", true).unwrap(), "MIXED");
    // Each `ß` grows from two bytes to two, `ŉ` from two to three.
    let growing = "ŉ".repeat(MAX_STRING_BYTES / 2);
    assert_eq!(
        change_case_checked(&growing, true).unwrap_err().code,
        "AU4005"
    );
}

#[test]
fn chars_lists_each_scalar() {
    let Value::Vec(list) = chars("aé").unwrap() else {
        panic!("a list");
    };
    assert_eq!(list.elements.len(), 2);
}
