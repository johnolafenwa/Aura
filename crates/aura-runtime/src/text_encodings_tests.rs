use super::{decode, encode, Encoding, EncodingError};

const ALL: [Encoding; 11] = [
    Encoding::Utf8,
    Encoding::Utf8Sig,
    Encoding::Utf16,
    Encoding::Utf16Le,
    Encoding::Utf16Be,
    Encoding::Utf32,
    Encoding::Utf32Le,
    Encoding::Utf32Be,
    Encoding::Latin1,
    Encoding::Ascii,
    Encoding::Windows1252,
];

#[test]
fn names_ignore_case_and_separators() {
    for (name, encoding) in [
        ("utf-8", Encoding::Utf8),
        ("UTF8", Encoding::Utf8),
        ("utf-8-sig", Encoding::Utf8Sig),
        ("UTF-16", Encoding::Utf16),
        ("utf_16_le", Encoding::Utf16Le),
        ("utf16be", Encoding::Utf16Be),
        ("utf-32", Encoding::Utf32),
        ("UTF-32LE", Encoding::Utf32Le),
        ("utf 32 be", Encoding::Utf32Be),
        ("latin-1", Encoding::Latin1),
        ("ISO-8859-1", Encoding::Latin1),
        ("l1", Encoding::Latin1),
        ("ascii", Encoding::Ascii),
        ("US-ASCII", Encoding::Ascii),
        ("windows-1252", Encoding::Windows1252),
        ("cp1252", Encoding::Windows1252),
    ] {
        assert_eq!(Encoding::from_name(name), Some(encoding), "{name}");
    }
    assert_eq!(Encoding::from_name("ebcdic"), None);
    assert_eq!(Encoding::from_name(""), None);
}

#[test]
fn ascii_text_round_trips_in_every_encoding() {
    let text = "Aura 0.17, plain text.";
    for encoding in ALL {
        let bytes = encode(text, encoding).expect("ASCII encodes everywhere");
        assert_eq!(
            decode(&bytes, encoding).as_deref(),
            Ok(text),
            "{encoding:?}"
        );
    }
}

#[test]
fn unicode_text_round_trips_in_the_unicode_encodings() {
    let text = "h\u{e9}llo \u{20ac} \u{1F30C}";
    for encoding in &ALL[..8] {
        let bytes = encode(text, *encoding).expect("Unicode encodes");
        assert_eq!(
            decode(&bytes, *encoding).as_deref(),
            Ok(text),
            "{encoding:?}"
        );
    }
}

#[test]
fn encodings_write_the_documented_bytes() {
    assert_eq!(
        encode("A\u{e9}", Encoding::Utf8),
        Ok(vec![0x41, 0xC3, 0xA9])
    );
    assert_eq!(
        encode("A", Encoding::Utf8Sig),
        Ok(vec![0xEF, 0xBB, 0xBF, 0x41])
    );
    assert_eq!(
        encode("A", Encoding::Utf16),
        Ok(vec![0xFF, 0xFE, 0x41, 0x00])
    );
    assert_eq!(encode("A", Encoding::Utf16Le), Ok(vec![0x41, 0x00]));
    assert_eq!(encode("A", Encoding::Utf16Be), Ok(vec![0x00, 0x41]));
    assert_eq!(
        encode("\u{1F30C}", Encoding::Utf16Be),
        Ok(vec![0xD8, 0x3C, 0xDF, 0x0C])
    );
    assert_eq!(
        encode("A", Encoding::Utf32),
        Ok(vec![0xFF, 0xFE, 0, 0, 0x41, 0, 0, 0])
    );
    assert_eq!(encode("A", Encoding::Utf32Le), Ok(vec![0x41, 0, 0, 0]));
    assert_eq!(encode("A", Encoding::Utf32Be), Ok(vec![0, 0, 0, 0x41]));
    assert_eq!(
        encode("\u{e9}\u{ff}", Encoding::Latin1),
        Ok(vec![0xE9, 0xFF])
    );
    assert_eq!(
        encode("\u{20ac}\u{e9}\u{178}", Encoding::Windows1252),
        Ok(vec![0x80, 0xE9, 0x9F])
    );
}

#[test]
fn encoding_reports_the_first_character_without_bytes() {
    assert_eq!(
        encode("ab\u{e9}", Encoding::Ascii),
        Err(EncodingError::Unencodable { index: 2 })
    );
    assert_eq!(
        encode("\u{e9}\u{20ac}", Encoding::Latin1),
        Err(EncodingError::Unencodable { index: 1 })
    );
    // U+0081 is a Latin-1 control that Windows-1252 leaves undefined.
    assert_eq!(
        encode("a\u{81}", Encoding::Windows1252),
        Err(EncodingError::Unencodable { index: 1 })
    );
    assert_eq!(
        encode("\u{1F30C}", Encoding::Windows1252),
        Err(EncodingError::Unencodable { index: 0 })
    );
}

#[test]
fn decoding_reads_byte_order_marks() {
    assert_eq!(
        decode(&[0xFE, 0xFF, 0x00, 0x41], Encoding::Utf16).as_deref(),
        Ok("A")
    );
    assert_eq!(
        decode(&[0xFF, 0xFE, 0x41, 0x00], Encoding::Utf16).as_deref(),
        Ok("A")
    );
    assert_eq!(decode(&[0x41, 0x00], Encoding::Utf16).as_deref(), Ok("A"));
    assert_eq!(
        decode(&[0, 0, 0xFE, 0xFF, 0, 0, 0, 0x41], Encoding::Utf32).as_deref(),
        Ok("A")
    );
    assert_eq!(
        decode(&[0xFF, 0xFE, 0, 0, 0x41, 0, 0, 0], Encoding::Utf32).as_deref(),
        Ok("A")
    );
    assert_eq!(
        decode(&[0x41, 0, 0, 0], Encoding::Utf32).as_deref(),
        Ok("A")
    );
    assert_eq!(
        decode(&[0xEF, 0xBB, 0xBF, 0x41], Encoding::Utf8Sig).as_deref(),
        Ok("A")
    );
    assert_eq!(decode(&[0x41], Encoding::Utf8Sig).as_deref(), Ok("A"));
    // A mark is text in the encodings that name their byte order.
    assert_eq!(
        decode(&[0xFF, 0xFE, 0x41, 0x00], Encoding::Utf16Le).as_deref(),
        Ok("\u{feff}A")
    );
}

#[test]
fn decoding_reports_the_offset_of_the_first_invalid_sequence() {
    let invalid = |index| Err(EncodingError::Invalid { index });
    assert_eq!(decode(&[0x41, 0xFF], Encoding::Utf8), invalid(1));
    assert_eq!(
        decode(&[0xEF, 0xBB, 0xBF, 0x41, 0xC3], Encoding::Utf8Sig),
        invalid(4)
    );
    assert_eq!(decode(&[0x41, 0x80], Encoding::Ascii), invalid(1));
    assert_eq!(decode(&[0x41, 0x81], Encoding::Windows1252), invalid(1));
    // Half a code unit, a lone low surrogate, and a high surrogate with
    // nothing or the wrong unit after it.
    assert_eq!(decode(&[0x41, 0x00, 0x42], Encoding::Utf16Le), invalid(2));
    assert_eq!(decode(&[0x00, 0xDC], Encoding::Utf16Le), invalid(0));
    assert_eq!(
        decode(&[0x41, 0x00, 0x00, 0xD8], Encoding::Utf16Le),
        invalid(2)
    );
    assert_eq!(
        decode(&[0x00, 0xD8, 0x41, 0x00], Encoding::Utf16Le),
        invalid(0)
    );
    assert_eq!(
        decode(&[0xFF, 0xFE, 0x00, 0xDC], Encoding::Utf16),
        invalid(2)
    );
    // A surrogate, a value past U+10FFFF, and a short final word.
    assert_eq!(decode(&[0x00, 0xD8, 0, 0], Encoding::Utf32Le), invalid(0));
    assert_eq!(
        decode(&[0, 0, 0, 0x41, 0x00, 0x11, 0, 0], Encoding::Utf32Be),
        invalid(4)
    );
    assert_eq!(
        decode(&[0x41, 0, 0, 0, 0x42], Encoding::Utf32Le),
        invalid(4)
    );
    assert_eq!(
        decode(&[0xFF, 0xFE, 0, 0, 0x41, 0], Encoding::Utf32),
        invalid(4)
    );
}

#[test]
fn single_byte_decoders_map_every_defined_byte() {
    let all: Vec<u8> = (0..=255).collect();
    let latin = decode(&all, Encoding::Latin1).expect("Latin-1 defines every byte");
    assert_eq!(latin.chars().count(), 256);
    assert_eq!(encode(&latin, Encoding::Latin1), Ok(all.clone()));
    let defined: Vec<u8> = all
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, 0x81 | 0x8D | 0x8F | 0x90 | 0x9D))
        .collect();
    let windows = decode(&defined, Encoding::Windows1252).expect("defined bytes decode");
    assert_eq!(encode(&windows, Encoding::Windows1252), Ok(defined));
    assert_eq!(
        decode(&[0x80], Encoding::Windows1252).as_deref(),
        Ok("\u{20ac}")
    );
}
