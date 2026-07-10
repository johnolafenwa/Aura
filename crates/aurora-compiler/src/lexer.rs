use crate::diag::{Diagnostic, Result, Span};
use crate::limits::RECURSION_LIMIT;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    IntLiteral(u128),
    DurationLiteral(i128),
    FloatLiteral(f64),
    BoolLiteral(bool),
    StringLiteral(String),
    FStringLiteral(String),
    Newline,
    Indent,
    Dedent,
    Eof,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Comma,
    Dot,
    Question,
    Equal,
    EqEq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Plus,
    PlusEqual,
    Minus,
    MinusEqual,
    Star,
    StarEqual,
    Slash,
    SlashEqual,
    Percent,
    PercentEqual,
    Arrow,
    KwClass,
    KwEnum,
    KwDef,
    KwTrait,
    KwImpl,
    KwImport,
    KwFrom,
    KwMut,
    KwBorrow,
    KwIndirect,
    KwPublic,
    KwReturn,
    KwIf,
    KwElif,
    KwElse,
    KwAnd,
    KwOr,
    KwNot,
    KwMatch,
    KwCase,
    KwFor,
    KwIn,
    KwWhile,
    KwBreak,
    KwContinue,
    KwPass,
    KwTry,
    KwWith,
    KwAs,
}

fn decode_hex_digit(ch: char) -> Option<u32> {
    match ch {
        '0'..='9' => Some((ch as u32) - ('0' as u32)),
        'a'..='f' => Some((ch as u32) - ('a' as u32) + 10),
        'A'..='F' => Some((ch as u32) - ('A' as u32) + 10),
        _ => None,
    }
}

fn decode_escape(
    chars: &[(usize, char)],
    escape_start: usize,
    line_no: usize,
    column: usize,
    literal_kind: &str,
) -> Result<(char, usize)> {
    let Some((_, escaped)) = chars.get(escape_start) else {
        return Err(Diagnostic::at(
            Span::new(line_no, column),
            format!("unterminated {} literal", literal_kind),
        ));
    };
    match escaped {
        'n' => Ok(('\n', escape_start + 1)),
        't' => Ok(('\t', escape_start + 1)),
        '"' => Ok(('"', escape_start + 1)),
        '\\' => Ok(('\\', escape_start + 1)),
        '0' => Ok(('\0', escape_start + 1)),
        'x' => {
            let Some((_, high)) = chars.get(escape_start + 1) else {
                return Err(Diagnostic::at(
                    Span::new(line_no, column),
                    format!("unsupported escape sequence `\\{}`", escaped),
                ));
            };
            let Some((_, low)) = chars.get(escape_start + 2) else {
                return Err(Diagnostic::at(
                    Span::new(line_no, column),
                    format!("unsupported escape sequence `\\{}`", escaped),
                ));
            };
            let Some(high) = decode_hex_digit(*high) else {
                return Err(Diagnostic::at(
                    Span::new(line_no, column),
                    "invalid hexadecimal escape sequence",
                ));
            };
            let Some(low) = decode_hex_digit(*low) else {
                return Err(Diagnostic::at(
                    Span::new(line_no, column),
                    "invalid hexadecimal escape sequence",
                ));
            };
            let value = ((high << 4) | low) as u8;
            Ok((char::from(value), escape_start + 3))
        }
        'u' => {
            if !matches!(chars.get(escape_start + 1), Some((_, '{'))) {
                return Err(Diagnostic::at(
                    Span::new(line_no, column),
                    "unicode escape sequences must use the form `\\u{...}`",
                ));
            }
            let mut scalar = 0u32;
            let mut saw_digit = false;
            let mut index = escape_start + 2;
            while let Some((_, candidate)) = chars.get(index) {
                if *candidate == '}' {
                    if !saw_digit {
                        return Err(Diagnostic::at(
                            Span::new(line_no, column),
                            "unicode escape sequences must include at least one hexadecimal digit",
                        ));
                    }
                    let Some(decoded) = char::from_u32(scalar) else {
                        return Err(Diagnostic::at(
                            Span::new(line_no, column),
                            "unicode escape sequence is out of range",
                        ));
                    };
                    return Ok((decoded, index + 1));
                }
                let Some(digit) = decode_hex_digit(*candidate) else {
                    return Err(Diagnostic::at(
                        Span::new(line_no, column),
                        "invalid unicode escape sequence",
                    ));
                };
                scalar = scalar
                    .checked_mul(16)
                    .and_then(|value| value.checked_add(digit))
                    .ok_or_else(|| {
                        Diagnostic::at(
                            Span::new(line_no, column),
                            "unicode escape sequence is out of range",
                        )
                    })?;
                saw_digit = true;
                index += 1;
            }
            Err(Diagnostic::at(
                Span::new(line_no, column),
                format!("unterminated {} literal", literal_kind),
            ))
        }
        other => Err(Diagnostic::at(
            Span::new(line_no, column),
            format!("unsupported escape sequence `\\{}`", other),
        )),
    }
}

pub fn lex(source: &str) -> Result<Vec<Token>> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut tokens = Vec::new();
    let mut indent_stack = vec![0usize];

    for (index, raw_line) in source.lines().enumerate() {
        let line_no = index + 1;

        if raw_line.contains('\t') {
            return Err(Diagnostic::at(
                Span::new(line_no, 1),
                "tabs are not supported for indentation",
            ));
        }

        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = raw_line.chars().take_while(|ch| *ch == ' ').count();
        let content = &raw_line[indent..];
        let current_indent = *indent_stack.last().unwrap();

        if indent > current_indent {
            indent_stack.push(indent);
            tokens.push(Token {
                kind: TokenKind::Indent,
                span: Span::new(line_no, 1),
            });
        } else if indent < current_indent {
            while indent < *indent_stack.last().unwrap() {
                indent_stack.pop();
                tokens.push(Token {
                    kind: TokenKind::Dedent,
                    span: Span::new(line_no, 1),
                });
            }

            if indent != *indent_stack.last().unwrap() {
                return Err(Diagnostic::at(
                    Span::new(line_no, 1),
                    "inconsistent indentation",
                ));
            }
        }

        tokenize_line(content, line_no, indent + 1, &mut tokens)?;
        tokens.push(Token {
            kind: TokenKind::Newline,
            span: Span::new(line_no, raw_line.len() + 1),
        });
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token {
            kind: TokenKind::Dedent,
            span: Span::new(source.lines().count() + 1, 1),
        });
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(source.lines().count() + 1, 1),
    });

    Ok(tokens)
}

fn tokenize_line(
    content: &str,
    line_no: usize,
    base_column: usize,
    tokens: &mut Vec<Token>,
) -> Result<()> {
    let chars: Vec<(usize, char)> = content.char_indices().collect();
    let mut index = 0;

    while index < chars.len() {
        let (offset, ch) = chars[index];
        let column = base_column + offset;

        match ch {
            ' ' => {
                index += 1;
            }
            '#' => break,
            '(' => {
                tokens.push(simple(TokenKind::LParen, line_no, column));
                index += 1;
            }
            ')' => {
                tokens.push(simple(TokenKind::RParen, line_no, column));
                index += 1;
            }
            '[' => {
                tokens.push(simple(TokenKind::LBracket, line_no, column));
                index += 1;
            }
            ']' => {
                tokens.push(simple(TokenKind::RBracket, line_no, column));
                index += 1;
            }
            '{' => {
                tokens.push(simple(TokenKind::LBrace, line_no, column));
                index += 1;
            }
            '}' => {
                tokens.push(simple(TokenKind::RBrace, line_no, column));
                index += 1;
            }
            ':' => {
                tokens.push(simple(TokenKind::Colon, line_no, column));
                index += 1;
            }
            ',' => {
                tokens.push(simple(TokenKind::Comma, line_no, column));
                index += 1;
            }
            '.' => {
                tokens.push(simple(TokenKind::Dot, line_no, column));
                index += 1;
            }
            '?' => {
                tokens.push(simple(TokenKind::Question, line_no, column));
                index += 1;
            }
            '=' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::EqEq, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Equal, line_no, column));
                    index += 1;
                }
            }
            '!' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::NotEq, line_no, column));
                    index += 2;
                } else {
                    return Err(Diagnostic::at(
                        Span::new(line_no, column),
                        "unexpected character `!`",
                    ));
                }
            }
            '<' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::LessEq, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Less, line_no, column));
                    index += 1;
                }
            }
            '>' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::GreaterEq, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Greater, line_no, column));
                    index += 1;
                }
            }
            '+' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::PlusEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Plus, line_no, column));
                    index += 1;
                }
            }
            '*' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::StarEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Star, line_no, column));
                    index += 1;
                }
            }
            '/' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::SlashEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Slash, line_no, column));
                    index += 1;
                }
            }
            '%' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::PercentEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Percent, line_no, column));
                    index += 1;
                }
            }
            '-' => {
                if let Some((_, '>')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::Arrow, line_no, column));
                    index += 2;
                } else if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::MinusEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Minus, line_no, column));
                    index += 1;
                }
            }
            'f' if matches!(chars.get(index + 1), Some((_, '"'))) => {
                index += 2;
                let mut value = String::new();
                let mut interpolation_depth = 0usize;
                let mut interpolation_in_string = false;
                let mut interpolation_escape = false;

                while index < chars.len() {
                    let (_, current) = chars[index];
                    if interpolation_depth == 0 && current == '"' {
                        break;
                    }
                    if interpolation_depth == 0
                        && current == '{'
                        && matches!(chars.get(index + 1), Some((_, '{')))
                    {
                        value.push('{');
                        value.push('{');
                        index += 2;
                        continue;
                    }
                    if interpolation_depth == 0
                        && current == '}'
                        && matches!(chars.get(index + 1), Some((_, '}')))
                    {
                        value.push('}');
                        value.push('}');
                        index += 2;
                        continue;
                    }
                    if interpolation_depth == 0 && current == '\\' {
                        let (decoded, next_index) =
                            decode_escape(&chars, index + 1, line_no, column, "f-string")?;
                        value.push(decoded);
                        index = next_index;
                        continue;
                    }
                    value.push(current);
                    if interpolation_depth > 0 {
                        if interpolation_in_string {
                            if interpolation_escape {
                                interpolation_escape = false;
                            } else if current == '\\' {
                                interpolation_escape = true;
                            } else if current == '"' {
                                interpolation_in_string = false;
                            }
                        } else {
                            match current {
                                '"' => interpolation_in_string = true,
                                '{' => {
                                    if interpolation_depth >= RECURSION_LIMIT {
                                        return Err(Diagnostic::at(
                                            Span::new(line_no, column),
                                            format!(
                                                "f-string interpolation exceeds the supported nesting limit of {}",
                                                RECURSION_LIMIT
                                            ),
                                        ));
                                    }
                                    interpolation_depth += 1;
                                }
                                '}' => interpolation_depth = interpolation_depth.saturating_sub(1),
                                _ => {}
                            }
                        }
                    } else if current == '{' {
                        interpolation_depth = 1;
                    }
                    index += 1;
                }

                if !matches!(chars.get(index), Some((_, '"'))) {
                    return Err(Diagnostic::at(
                        Span::new(line_no, column),
                        "unterminated f-string literal",
                    ));
                }

                index += 1;
                tokens.push(Token {
                    kind: TokenKind::FStringLiteral(value),
                    span: Span::new(line_no, column),
                });
            }
            '"' => {
                index += 1;
                let mut value = String::new();

                while index < chars.len() {
                    let (_, current) = chars[index];
                    if current == '"' {
                        break;
                    }
                    if current == '\\' {
                        let (decoded, next_index) =
                            decode_escape(&chars, index + 1, line_no, column, "string")?;
                        value.push(decoded);
                        index = next_index;
                        continue;
                    }
                    value.push(current);
                    index += 1;
                }

                if !matches!(chars.get(index), Some((_, '"'))) {
                    return Err(Diagnostic::at(
                        Span::new(line_no, column),
                        "unterminated string literal",
                    ));
                }

                index += 1;
                tokens.push(Token {
                    kind: TokenKind::StringLiteral(value),
                    span: Span::new(line_no, column),
                });
            }
            '0'..='9' => {
                let start = index;
                index += 1;

                while matches!(chars.get(index), Some((_, '0'..='9'))) {
                    index += 1;
                }

                let mut is_float = false;
                if matches!(chars.get(index), Some((_, '.')))
                    && matches!(chars.get(index + 1), Some((_, '0'..='9')))
                {
                    is_float = true;
                    index += 1;

                    while matches!(chars.get(index), Some((_, '0'..='9'))) {
                        index += 1;
                    }
                }

                if matches!(chars.get(index), Some((_, 'e' | 'E'))) {
                    let exponent_start = index;
                    let mut exponent_index = index + 1;
                    if matches!(chars.get(exponent_index), Some((_, '+' | '-'))) {
                        exponent_index += 1;
                    }
                    if !matches!(chars.get(exponent_index), Some((_, '0'..='9'))) {
                        return Err(Diagnostic::at(
                            Span::new(line_no, base_column + chars[exponent_start].0),
                            "invalid floating-point literal",
                        ));
                    }
                    is_float = true;
                    index = exponent_index + 1;
                    while matches!(chars.get(index), Some((_, '0'..='9'))) {
                        index += 1;
                    }
                }

                let end_offset = match chars.get(index) {
                    Some((next_offset, _)) => *next_offset,
                    None => content.len(),
                };
                let text = &content[chars[start].0..end_offset];

                if is_float {
                    let value = text
                        .parse::<f64>()
                        .expect("lexer should only build syntactically valid float literals");
                    if !value.is_finite() {
                        return Err(Diagnostic::at(
                            Span::new(line_no, column),
                            "floating-point literal is out of range",
                        ));
                    }
                    tokens.push(Token {
                        kind: TokenKind::FloatLiteral(value),
                        span: Span::new(line_no, column),
                    });
                } else {
                    let value = match text.parse::<u128>() {
                        Ok(value) => value,
                        Err(_) => {
                            return Err(Diagnostic::at(
                                Span::new(line_no, column),
                                "invalid integer literal",
                            ));
                        }
                    };
                    let duration_kind = if let Some((_, suffix_start)) = chars.get(index) {
                        match suffix_start {
                            'm' => {
                                if matches!(chars.get(index + 1), Some((_, 's'))) {
                                    index += 2;
                                    Some(TokenKind::DurationLiteral(match i128::try_from(value) {
                                        Ok(value) => value,
                                        Err(_) => {
                                            return Err(Diagnostic::at(
                                                Span::new(line_no, column),
                                                "invalid duration literal",
                                            ));
                                        }
                                    }))
                                } else {
                                    index += 1;
                                    let multiplied = match value.checked_mul(60_000) {
                                        Some(value) => value,
                                        None => {
                                            return Err(Diagnostic::at(
                                                Span::new(line_no, column),
                                                "invalid duration literal",
                                            ));
                                        }
                                    };
                                    Some(TokenKind::DurationLiteral(
                                        match i128::try_from(multiplied) {
                                            Ok(value) => value,
                                            Err(_) => {
                                                return Err(Diagnostic::at(
                                                    Span::new(line_no, column),
                                                    "invalid duration literal",
                                                ));
                                            }
                                        },
                                    ))
                                }
                            }
                            's' => {
                                index += 1;
                                let multiplied = match value.checked_mul(1000) {
                                    Some(value) => value,
                                    None => {
                                        return Err(Diagnostic::at(
                                            Span::new(line_no, column),
                                            "invalid duration literal",
                                        ));
                                    }
                                };
                                Some(TokenKind::DurationLiteral(
                                    match i128::try_from(multiplied) {
                                        Ok(value) => value,
                                        Err(_) => {
                                            return Err(Diagnostic::at(
                                                Span::new(line_no, column),
                                                "invalid duration literal",
                                            ));
                                        }
                                    },
                                ))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    tokens.push(Token {
                        kind: duration_kind.unwrap_or(TokenKind::IntLiteral(value)),
                        span: Span::new(line_no, column),
                    });
                }
            }
            '_' | 'a'..='z' | 'A'..='Z' => {
                let start = index;
                index += 1;

                while matches!(
                    chars.get(index),
                    Some((_, '_' | 'a'..='z' | 'A'..='Z' | '0'..='9'))
                ) {
                    index += 1;
                }

                let end_offset = chars
                    .get(index)
                    .map(|(next_offset, _)| *next_offset)
                    .unwrap_or_else(|| content.len());
                let text = &content[chars[start].0..end_offset];
                let kind = match text {
                    "class" => TokenKind::KwClass,
                    "enum" => TokenKind::KwEnum,
                    "def" => TokenKind::KwDef,
                    "trait" => TokenKind::KwTrait,
                    "impl" => TokenKind::KwImpl,
                    "import" => TokenKind::KwImport,
                    "from" => TokenKind::KwFrom,
                    "mut" => TokenKind::KwMut,
                    "borrow" => TokenKind::KwBorrow,
                    "indirect" => TokenKind::KwIndirect,
                    "public" => TokenKind::KwPublic,
                    "return" => TokenKind::KwReturn,
                    "if" => TokenKind::KwIf,
                    "elif" => TokenKind::KwElif,
                    "else" => TokenKind::KwElse,
                    "and" => TokenKind::KwAnd,
                    "or" => TokenKind::KwOr,
                    "not" => TokenKind::KwNot,
                    "match" => TokenKind::KwMatch,
                    "case" => TokenKind::KwCase,
                    "for" => TokenKind::KwFor,
                    "in" => TokenKind::KwIn,
                    "while" => TokenKind::KwWhile,
                    "break" => TokenKind::KwBreak,
                    "continue" => TokenKind::KwContinue,
                    "pass" => TokenKind::KwPass,
                    "try" => TokenKind::KwTry,
                    "with" => TokenKind::KwWith,
                    "as" => TokenKind::KwAs,
                    "true" => TokenKind::BoolLiteral(true),
                    "false" => TokenKind::BoolLiteral(false),
                    _ => TokenKind::Identifier(text.to_string()),
                };
                tokens.push(Token {
                    kind,
                    span: Span::new(line_no, column),
                });
            }
            _ => {
                return Err(Diagnostic::at(
                    Span::new(line_no, column),
                    format!("unexpected character `{}`", ch),
                ));
            }
        }
    }

    Ok(())
}

fn simple(kind: TokenKind, line: usize, column: usize) -> Token {
    Token {
        kind,
        span: Span::new(line, column),
    }
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;
