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
            "class enum def trait impl import from mut borrow indirect public return if elif else and or not match case for in while break continue pass try with as select spawn detached true false name ? ( ) [ ] { } : , . = == != < <= > >= + += * *= / /= % %= - -> -=\n",
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
    assert!(tokens.contains(&TokenKind::KwWhile));
    assert!(tokens.contains(&TokenKind::KwBreak));
    assert!(tokens.contains(&TokenKind::KwContinue));
    assert!(tokens.contains(&TokenKind::KwPass));
    assert!(tokens.contains(&TokenKind::KwTry));
    assert!(tokens.contains(&TokenKind::KwWith));
    assert!(tokens.contains(&TokenKind::KwAs));
    assert!(tokens.contains(&TokenKind::KwSelect));
    assert!(tokens.contains(&TokenKind::KwSpawn));
    assert!(tokens.contains(&TokenKind::KwDetached));
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
    assert!(tokens.contains(&TokenKind::Percent));
    assert!(tokens.contains(&TokenKind::PercentEqual));
    assert!(tokens.contains(&TokenKind::Minus));
    assert!(tokens.contains(&TokenKind::Arrow));
    assert!(tokens.contains(&TokenKind::MinusEqual));
}

#[test]
fn lexes_strings_fstrings_numbers_and_durations() {
    let tokens = kinds(
            "value = \"line\\ntext\\t\\\"quote\\\"\\\\\"\nmessage = f\"hello {user}\"\nnums = 7 1.5 5ms 2s 1m\n",
        );

    assert!(tokens.contains(&TokenKind::StringLiteral(
        "line\ntext\t\"quote\"\\".to_string()
    )));
    assert!(tokens.contains(&TokenKind::FStringLiteral("hello {user}".to_string())));
    assert!(tokens.contains(&TokenKind::IntLiteral(7)));
    assert!(tokens.contains(&TokenKind::FloatLiteral(1.5)));
    assert!(tokens.contains(&TokenKind::DurationLiteral(5)));
    assert!(tokens.contains(&TokenKind::DurationLiteral(2_000)));
    assert!(tokens.contains(&TokenKind::DurationLiteral(60_000)));
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

    let invalid = lex("value = @\n").unwrap_err();
    assert!(invalid.message.contains("unexpected character `@`"));
}

#[test]
fn lexer_reports_fstring_interpolation_depth_limit() {
    let depth = crate::limits::RECURSION_LIMIT + 8;
    let mut source = "text = f\"".to_string();
    source.push('{');
    source.push_str(&"{".repeat(depth));
    source.push('1');
    source.push_str(&"}".repeat(depth));
    source.push('}');
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
