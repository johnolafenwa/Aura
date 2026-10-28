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
