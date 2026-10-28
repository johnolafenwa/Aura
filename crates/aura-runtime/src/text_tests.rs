use super::Text;

#[test]
fn the_flag_follows_every_change() {
    let mut text = Text::from("abc");
    assert!(text.is_ascii());
    assert_eq!(text.char_len(), 3);
    text.push('é');
    assert!(!text.is_ascii());
    assert_eq!(text.char_len(), 4);
    assert_eq!(text.slice(0..2), Text::from("ab"));
    assert!(text.slice(0..2).is_ascii());
    text.clear();
    assert!(text.is_ascii());
    text.push_str("ü");
    assert!(!text.is_ascii());
    let mut joined = Text::new();
    joined.push_text(&Text::from("x"));
    assert!(joined.is_ascii());
    joined.push_text(&Text::from("ß"));
    assert!(!joined.is_ascii());
    assert_eq!(String::from(joined.clone()), "xß");
    assert_eq!(format!("{joined:?} {joined}"), "\"xß\" xß");
    assert_eq!(['a', 'b'].into_iter().collect::<Text>(), "ab");
    assert_eq!(Text::from('z'), Text::from(&"z".to_string()));
}

#[test]
fn a_short_text_stays_inline_until_it_outgrows_the_value() {
    assert_eq!(std::mem::size_of::<Text>(), 32);
    let mut text = Text::from("x".repeat(super::INLINE_CAPACITY).as_str());
    assert_eq!(text.capacity(), super::INLINE_CAPACITY);
    text.reserve(0);
    assert_eq!(text.capacity(), super::INLINE_CAPACITY);
    text.push('y');
    assert!(text.capacity() > super::INLINE_CAPACITY);
    assert_eq!(text.len(), super::INLINE_CAPACITY + 1);
    assert!(text.ends_with("xy"));
    // A copy of a short heap text, or a short part of one, is inline again.
    let long = Text::from("abcdefghij".repeat(4));
    let part = long.slice(2..5);
    assert_eq!(
        (part.as_str(), part.capacity()),
        ("cde", super::INLINE_CAPACITY)
    );
    let short = Text::from(String::from("hi"));
    assert_eq!(short.clone().capacity(), super::INLINE_CAPACITY);
    assert_eq!(short.clone().into_string(), "hi");
    let mut grown = Text::new();
    grown
        .try_reserve(100)
        .expect("a small reservation succeeds");
    assert!(grown.capacity() >= 100);
    grown.push_str("é");
    assert!(!grown.is_ascii());
    grown.clear();
    assert!(grown.is_ascii() && grown.is_empty());
    let mut inline = Text::from("ab");
    inline.clear();
    assert_eq!((inline.as_str(), inline.is_ascii()), ("", true));
    assert_eq!(Text::from('é').char_len(), 1);
    assert_eq!(Text::default(), "");
}

#[test]
fn a_text_converts_compares_and_formats_as_its_bytes() {
    use std::collections::HashSet;
    let owned = String::from("path/name");
    let text = Text::from(&owned);
    assert!(text == owned && text == "path/name" && text == *"path/name");
    let path: &std::path::Path = text.as_ref();
    assert_eq!(path, std::path::Path::new("path/name"));
    let os: &std::ffi::OsStr = text.as_ref();
    assert_eq!(os, "path/name");
    let bytes: &[u8] = text.as_ref();
    assert_eq!(bytes, b"path/name");
    assert_eq!(AsRef::<str>::as_ref(&text), "path/name");
    let borrowed: &str = std::borrow::Borrow::borrow(&text);
    assert_eq!(borrowed, "path/name");
    assert_eq!(format!("{text:?} {text}"), "\"path/name\" path/name");
    assert!(Text::from("a") < Text::from("b"));
    assert_eq!(
        Text::from("b").partial_cmp(&Text::from("a")),
        Some(std::cmp::Ordering::Greater)
    );
    let set: HashSet<Text> = ["x", "x", "y"].into_iter().map(Text::from).collect();
    assert!(set.contains("x") && set.len() == 2);
    let collected: Text = "héllo".chars().collect();
    assert_eq!((collected.char_len(), collected.is_ascii()), (5, false));
    assert_eq!(String::from(Text::from("é")), "é");
    let mut pushed = Text::from("a");
    pushed.push('é');
    pushed.push_text(&Text::from("b"));
    pushed.reserve(64);
    assert_eq!(pushed.as_str(), "aéb");
    assert!(!pushed.is_ascii() && pushed.capacity() >= 64);
}

#[test]
fn joined_and_case_mapped_texts_keep_their_flag_and_stay_inline_when_short() {
    let short = Text::concat(&Text::from("ab"), &Text::from("cd"));
    assert_eq!(
        (short.as_str(), short.is_ascii(), short.capacity()),
        ("abcd", true, super::INLINE_CAPACITY)
    );
    let long = Text::concat(&Text::from("x".repeat(20)), &Text::from("é".repeat(10)));
    assert_eq!(long.len(), 40);
    assert!(!long.is_ascii());
    assert_eq!(long.char_len(), 30);
    let upper = Text::from("MiXeD").map_ascii(u8::to_ascii_lowercase);
    assert_eq!(upper.as_deref(), Some("mixed"));
    let wide = Text::from("a".repeat(40)).map_ascii(u8::to_ascii_uppercase);
    assert_eq!(wide.map(Text::into_string), Some("A".repeat(40)));
    assert!(Text::from("é").map_ascii(u8::to_ascii_uppercase).is_none());
}
