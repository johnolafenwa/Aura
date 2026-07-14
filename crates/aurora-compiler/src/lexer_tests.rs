use super::{lex, TokenKind};
use crate::diag::Span;

fn kinds(source: &str) -> Vec<TokenKind> {
    lex(source)
        .unwrap()
        .into_iter()
        .map(|token| token.kind)
        .collect()
}

#[test]
fn lexes_keywords_operators_and_delimiters() {
    let tokens = kinds(
            "class enum def trait impl import from mut borrow indirect public return if elif else and or not match case for in is while break continue pass try with as select spawn detached true false name ? ( ) [ ] { } : , . = == != < <= > >= + += * *= / /= // //= % %= - -> -=\n",
        );

    assert!(tokens.contains(&TokenKind::KwClass));
    assert!(tokens.contains(&TokenKind::KwEnum));
    assert!(tokens.contains(&TokenKind::KwDef));
    assert!(tokens.contains(&TokenKind::KwTrait));
    assert!(tokens.contains(&TokenKind::KwImpl));
    assert!(tokens.contains(&TokenKind::KwImport));
    assert!(tokens.contains(&TokenKind::KwFrom));
    assert!(tokens.contains(&TokenKind::KwMut));
    assert!(tokens.contains(&TokenKind::KwBorrow));
    assert!(tokens.contains(&TokenKind::KwIndirect));
    assert!(tokens.contains(&TokenKind::KwPublic));
    assert!(tokens.contains(&TokenKind::KwReturn));
    assert!(tokens.contains(&TokenKind::KwIf));
    assert!(tokens.contains(&TokenKind::KwElif));
    assert!(tokens.contains(&TokenKind::KwElse));
    assert!(tokens.contains(&TokenKind::KwAnd));
    assert!(tokens.contains(&TokenKind::KwOr));
    assert!(tokens.contains(&TokenKind::KwNot));
    assert!(tokens.contains(&TokenKind::KwMatch));
    assert!(tokens.contains(&TokenKind::KwCase));
    assert!(tokens.contains(&TokenKind::KwFor));
    assert!(tokens.contains(&TokenKind::KwIn));
    assert!(tokens.contains(&TokenKind::KwIs));
    assert!(tokens.contains(&TokenKind::KwWhile));
    assert!(tokens.contains(&TokenKind::KwBreak));
    assert!(tokens.contains(&TokenKind::KwContinue));
    assert!(tokens.contains(&TokenKind::KwPass));
    assert!(tokens.contains(&TokenKind::KwTry));
    assert!(tokens.contains(&TokenKind::KwWith));
    assert!(tokens.contains(&TokenKind::KwAs));
    assert!(tokens.contains(&TokenKind::Identifier("select".to_string())));
    assert!(tokens.contains(&TokenKind::Identifier("spawn".to_string())));
    assert!(tokens.contains(&TokenKind::Identifier("detached".to_string())));
    assert!(tokens.contains(&TokenKind::BoolLiteral(true)));
    assert!(tokens.contains(&TokenKind::BoolLiteral(false)));
    assert!(tokens.contains(&TokenKind::Identifier("name".to_string())));
    assert!(tokens.contains(&TokenKind::Question));
    assert!(tokens.contains(&TokenKind::LParen));
    assert!(tokens.contains(&TokenKind::RParen));
    assert!(tokens.contains(&TokenKind::LBracket));
    assert!(tokens.contains(&TokenKind::RBracket));
    assert!(tokens.contains(&TokenKind::LBrace));
    assert!(tokens.contains(&TokenKind::RBrace));
    assert!(tokens.contains(&TokenKind::Colon));
    assert!(tokens.contains(&TokenKind::Comma));
    assert!(tokens.contains(&TokenKind::Dot));
    assert!(tokens.contains(&TokenKind::Equal));
    assert!(tokens.contains(&TokenKind::EqEq));
    assert!(tokens.contains(&TokenKind::NotEq));
    assert!(tokens.contains(&TokenKind::Less));
    assert!(tokens.contains(&TokenKind::LessEq));
    assert!(tokens.contains(&TokenKind::Greater));
    assert!(tokens.contains(&TokenKind::GreaterEq));
    assert!(tokens.contains(&TokenKind::Plus));
    assert!(tokens.contains(&TokenKind::PlusEqual));
    assert!(tokens.contains(&TokenKind::Star));
    assert!(tokens.contains(&TokenKind::StarEqual));
    assert!(tokens.contains(&TokenKind::Slash));
    assert!(tokens.contains(&TokenKind::SlashEqual));
    assert!(tokens.contains(&TokenKind::DoubleSlash));
    assert!(tokens.contains(&TokenKind::DoubleSlashEqual));
    assert!(tokens.contains(&TokenKind::Percent));
    assert!(tokens.contains(&TokenKind::PercentEqual));
    assert!(tokens.contains(&TokenKind::Minus));
    assert!(tokens.contains(&TokenKind::Arrow));
    assert!(tokens.contains(&TokenKind::MinusEqual));
}

#[test]
fn lexes_strings_fstrings_numbers_and_durations() {
    let tokens = kinds(
            "value = \"line\\ntext\\t\\\"quote\\\"\\\\\"\nmessage = f\"hello {user}\"\nnums = 7 1.5 1e3 2.5e-1 5ms 2s 1m\n",
        );

    assert!(tokens.contains(&TokenKind::StringLiteral(
        "line\ntext\t\"quote\"\\".to_string()
    )));
    assert!(tokens.contains(&TokenKind::FStringLiteral("hello {user}".to_string())));
    assert!(tokens.contains(&TokenKind::IntLiteral(7)));
    assert!(tokens.contains(&TokenKind::FloatLiteral(1.5)));
    assert!(tokens.contains(&TokenKind::FloatLiteral(1000.0)));
    assert!(tokens.contains(&TokenKind::FloatLiteral(0.25)));
    assert!(tokens.contains(&TokenKind::DurationLiteral(5)));
    assert!(tokens.contains(&TokenKind::DurationLiteral(2_000)));
    assert!(tokens.contains(&TokenKind::DurationLiteral(60_000)));
}

#[test]
fn d4_lexer_accepts_single_quoted_strings_with_shared_escape_semantics() {
    let tokens = lex(concat!(
        r#"text = 'line\ntext\t\"double\"\'single\'\\\0\x41\u{1F600}'"#,
        "\n",
        r#"double = "it\'s valid""#,
        "\n",
        r#"literal = 'a "quote" and # text'"#,
        "\n",
    ))
    .expect("both ordinary string delimiters should lex");

    let expected = "line\ntext\t\"double\"'single'\\\0A😀".to_string();
    let text = tokens
        .iter()
        .find(|token| token.kind == TokenKind::StringLiteral(expected.clone()))
        .expect("single-quoted escape set should decode like the double-quoted form");
    assert_eq!(text.span, Span::new(1, 8));
    assert!(tokens
        .iter()
        .any(|token| token.kind == TokenKind::StringLiteral("it's valid".to_string())));
    assert!(tokens.iter().any(|token| {
        token.kind == TokenKind::StringLiteral("a \"quote\" and # text".to_string())
    }));
}

#[test]
fn d4_lexer_reports_single_quoted_string_diagnostics_at_the_literal_span() {
    let single_escape = lex(r#"text = '\q'"#).expect_err("unknown escape should fail");
    let double_escape = lex(r#"text = "\q""#).expect_err("unknown escape should fail");
    assert_eq!(single_escape.message, double_escape.message);
    assert_eq!(single_escape.span, Some(Span::new(1, 8)));

    let single_hex = lex(r#"text = '\x4g'"#).expect_err("bad hex escape should fail");
    let double_hex = lex(r#"text = "\x4g""#).expect_err("bad hex escape should fail");
    assert_eq!(single_hex.message, double_hex.message);
    assert_eq!(single_hex.span, Some(Span::new(1, 8)));

    let unterminated = lex("text = 'abc\\").expect_err("dangling escape should fail");
    assert_eq!(unterminated.span, Some(Span::new(1, 8)));
    assert!(unterminated.message.contains("unterminated string literal"));
}

#[test]
fn d4_lexer_tracks_single_quoted_strings_inside_fstring_interpolations() {
    let tokens = kinds("message = f\"{echo('{left')}\"\n");
    assert!(tokens.contains(&TokenKind::FStringLiteral("{echo('{left')}".to_string())));
}

#[test]
fn lexes_indentation_and_skips_blank_or_comment_lines() {
    let tokens = kinds(
            "def main():\n    value = 1\n\n    # comment only\n    if true:\n        print(value)\n    print(value)\n",
        );

    let indent_count = tokens
        .iter()
        .filter(|kind| **kind == TokenKind::Indent)
        .count();
    let dedent_count = tokens
        .iter()
        .filter(|kind| **kind == TokenKind::Dedent)
        .count();
    let newline_count = tokens
        .iter()
        .filter(|kind| **kind == TokenKind::Newline)
        .count();

    assert_eq!(indent_count, 2);
    assert_eq!(dedent_count, 2);
    assert_eq!(newline_count, 5);
}

#[test]
fn lexes_single_token_and_trailing_comment_cases() {
    for (source, expected) in [
        ("(\n", TokenKind::LParen),
        (")\n", TokenKind::RParen),
        ("[\n", TokenKind::LBracket),
        ("]\n", TokenKind::RBracket),
        ("{\n", TokenKind::LBrace),
        ("}\n", TokenKind::RBrace),
        (":\n", TokenKind::Colon),
        (",\n", TokenKind::Comma),
        (".\n", TokenKind::Dot),
        ("?\n", TokenKind::Question),
        ("=\n", TokenKind::Equal),
        ("==\n", TokenKind::EqEq),
        ("!=\n", TokenKind::NotEq),
        ("<\n", TokenKind::Less),
        ("<=\n", TokenKind::LessEq),
        (">\n", TokenKind::Greater),
        (">=\n", TokenKind::GreaterEq),
        ("+\n", TokenKind::Plus),
        ("+=\n", TokenKind::PlusEqual),
        ("-\n", TokenKind::Minus),
        ("->\n", TokenKind::Arrow),
        ("-=\n", TokenKind::MinusEqual),
        ("*\n", TokenKind::Star),
        ("*=\n", TokenKind::StarEqual),
        ("/\n", TokenKind::Slash),
        ("/=\n", TokenKind::SlashEqual),
        ("%\n", TokenKind::Percent),
        ("%=\n", TokenKind::PercentEqual),
    ] {
        let tokens = kinds(source);
        assert_eq!(tokens[0], expected, "failed for source {:?}", source);
    }

    let tokens = kinds("value # trailing comment\n");
    assert_eq!(tokens[0], TokenKind::Identifier("value".to_string()));
    assert!(tokens.contains(&TokenKind::Newline));

    let tokens = kinds("flag = true false\n");
    assert!(tokens.contains(&TokenKind::BoolLiteral(true)));
    assert!(tokens.contains(&TokenKind::BoolLiteral(false)));
}

#[test]
fn reports_lex_errors_for_tabs_bad_indentation_and_invalid_sequences() {
    let tab_error = lex("def main():\n\tprint(1)\n").unwrap_err();
    assert!(tab_error.message.contains("tabs are not supported"));

    let indent_error = lex("def main():\n    print(1)\n  print(2)\n").unwrap_err();
    assert!(indent_error.message.contains("inconsistent indentation"));

    let bang_error = lex("def main():\n    value = !true\n").unwrap_err();
    assert!(bang_error.message.contains("unexpected character `!`"));

    let string_escape_error = lex("def main():\n    text = \"\\q\"\n").unwrap_err();
    assert!(string_escape_error
        .message
        .contains("unsupported escape sequence `\\q`"));

    let fstring_escape_error = lex("def main():\n    text = f\"\\q\"\n").unwrap_err();
    assert!(fstring_escape_error
        .message
        .contains("unsupported escape sequence `\\q`"));

    let unterminated_string = lex("def main():\n    text = \"unterminated\n").unwrap_err();
    assert!(unterminated_string
        .message
        .contains("unterminated string literal"));

    let unterminated_fstring = lex("def main():\n    text = f\"unterminated\n").unwrap_err();
    assert!(unterminated_fstring
        .message
        .contains("unterminated f-string literal"));
}

#[test]
fn lexes_nested_interpolations_and_reports_duration_overflow() {
    let tokens = kinds("text = f\"value={items[0][1]} and {greet(name=\"World\")}\"\nwait = 42m\n");
    assert!(tokens.contains(&TokenKind::FStringLiteral(
        "value={items[0][1]} and {greet(name=\"World\")}".to_string()
    )));
    assert!(tokens.contains(&TokenKind::DurationLiteral(2_520_000)));

    let overflow = lex(&format!("value = {}m\n", u128::MAX)).unwrap_err();
    assert!(overflow.message.contains("invalid duration literal"));

    let float_overflow = lex("value = 1e1000\n").unwrap_err();
    assert!(float_overflow.message.contains("out of range"));

    let invalid = lex("value = @\n").unwrap_err();
    assert!(invalid.message.contains("unexpected character `@`"));
}

#[test]
fn lexer_ignores_a_utf8_bom_at_the_start_of_the_source() {
    let tokens = kinds("\u{feff}def main() -> int32:\n    return 0\n");
    assert_eq!(tokens[0], TokenKind::KwDef);
}

#[test]
fn lexer_reports_fstring_interpolation_depth_limit() {
    let depth = crate::limits::RECURSION_LIMIT + 8;
    let mut source = "text = f\"".to_string();
    source.push('{');
    source.push('0');
    source.push_str(&"{".repeat(depth));
    source.push('1');
    source.push_str(&"}".repeat(depth + 1));
    source.push_str("\"\n");

    let error = lex(&source).expect_err("deep f-string interpolation should fail");
    assert!(error.message.contains("nesting limit"));
}

#[test]
fn lexer_covers_successful_escape_decoding_and_signed_duration_range_failures() {
    let tokens = kinds(concat!(
        "text = \"\\n\\t\\\"\\\\\"\n",
        "value = f\"prefix {call(text=\"W\\\"orld\")} suffix\"\n",
        "escaped = f\"\\n\\t\\\"\\\\\"\n",
        "keep = 1ms\n",
    ));
    assert!(tokens.contains(&TokenKind::StringLiteral("\n\t\"\\".to_string())));
    assert!(tokens.contains(&TokenKind::FStringLiteral(
        "prefix {call(text=\"W\\\"orld\")} suffix".to_string()
    )));
    assert!(tokens.contains(&TokenKind::FStringLiteral("\n\t\"\\".to_string())));
    assert!(tokens.contains(&TokenKind::DurationLiteral(1)));

    let invalid_ms = lex(&format!("value = {}ms\n", (i128::MAX as u128) + 1))
        .expect_err("millisecond duration outside signed range should fail");
    assert!(invalid_ms.message.contains("invalid duration literal"));

    let invalid_s = lex(&format!("value = {}s\n", (i128::MAX as u128 / 1000) + 1))
        .expect_err("second duration outside signed range should fail");
    assert!(invalid_s.message.contains("invalid duration literal"));

    let invalid_m = lex(&format!("value = {}m\n", (i128::MAX as u128 / 60_000) + 1))
        .expect_err("minute duration outside signed range should fail");
    assert!(invalid_m.message.contains("invalid duration literal"));

    let checked_mul_overflow_s =
        lex(&format!("value = {}s\n", u128::MAX)).expect_err("second duration multiply overflow");
    assert!(checked_mul_overflow_s
        .message
        .contains("invalid duration literal"));

    let checked_mul_overflow_m =
        lex(&format!("value = {}m\n", u128::MAX)).expect_err("minute duration multiply overflow");
    assert!(checked_mul_overflow_m
        .message
        .contains("invalid duration literal"));

    let invalid_int =
        lex(&format!("value = {}0\n", u128::MAX)).expect_err("oversized integers should fail");
    assert!(invalid_int.message.contains("invalid integer literal"));
}

#[test]
fn lexer_reports_precise_spans_for_dedents_eof_and_errors() {
    let tokens = lex("def main():\n    if true:\n        pass\n").expect("source should lex");
    assert_eq!(tokens[tokens.len() - 2].kind, TokenKind::Dedent);
    assert_eq!(tokens[tokens.len() - 2].span, Span::new(4, 1));
    assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
    assert_eq!(tokens.last().unwrap().span, Span::new(4, 1));

    let unterminated_escape = lex("text = \"abc\\").expect_err("dangling escapes should fail");
    assert_eq!(unterminated_escape.span, Some(Span::new(1, 8)));
    assert!(unterminated_escape
        .message
        .contains("unterminated string literal"));

    let unterminated_fescape =
        lex("text = f\"abc\\").expect_err("dangling f-string escapes should fail");
    assert_eq!(unterminated_fescape.span, Some(Span::new(1, 8)));
    assert!(unterminated_fescape
        .message
        .contains("unterminated f-string literal"));
}

#[test]
fn lexer_decodes_extended_string_escape_sequences() {
    let tokens = kinds("text = \"\\0\\x41\\u{1F600}\"\nmessage = f\"\\x42\\u{43}\"\n");
    assert!(tokens.contains(&TokenKind::StringLiteral("\0A😀".to_string())));
    assert!(tokens.contains(&TokenKind::FStringLiteral("BC".to_string())));

    let bad_hex = lex("text = \"\\x4g\"\n").expect_err("invalid hex escape should fail");
    assert!(bad_hex
        .message
        .contains("invalid hexadecimal escape sequence"));

    let bad_unicode =
        lex("text = \"\\u{110000}\"\n").expect_err("out-of-range unicode escape should fail");
    assert!(bad_unicode.message.contains("out of range"));
}

#[test]
fn lexer_covers_extended_escape_brace_float_and_identifier_edges() {
    let tokens = kinds(concat!(
        "text = \"\\xAf\\u{ab}\\u{DE}\"\n",
        "message = f\"literal }} and escaped {{ plus {call(text=\"a\\\\\\\"b\")}\"\n",
        "floats = 1e+3 2E4 3. 4\n",
        "names = _value Camel1\n",
    ));
    assert!(tokens.contains(&TokenKind::StringLiteral("\u{af}\u{ab}\u{de}".to_string())));
    assert!(tokens.contains(&TokenKind::FStringLiteral(
        "literal }} and escaped {{ plus {call(text=\"a\\\\\\\"b\")}".to_string()
    )));
    assert!(tokens.contains(&TokenKind::FloatLiteral(1000.0)));
    assert!(tokens.contains(&TokenKind::FloatLiteral(20_000.0)));
    assert!(tokens.contains(&TokenKind::IntLiteral(3)));
    assert!(tokens.contains(&TokenKind::Dot));
    assert!(tokens.contains(&TokenKind::Identifier("_value".to_string())));
    assert!(tokens.contains(&TokenKind::Identifier("Camel1".to_string())));

    for source in [
        "text = \"\\x\"\n",
        "text = \"\\x4\"\n",
        "text = \"\\u\"\n",
        "text = \"\\u{}\"\n",
        "text = \"\\u{gg}\"\n",
        "text = \"\\u{FFFFFFFFF}\"\n",
        "value = 1e+\n",
    ] {
        let error = lex(source).expect_err("malformed literal should fail");
        assert!(
            error.message.contains("escape")
                || error.message.contains("literal")
                || error.message.contains("out of range"),
            "unexpected diagnostic for {source:?}: {}",
            error.message
        );
    }
}
