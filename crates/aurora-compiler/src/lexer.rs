use crate::diag::{Diagnostic, Result, Span};

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
    KwSelect,
    KwSpawn,
    KwDetached,
}

pub fn lex(source: &str) -> Result<Vec<Token>> {
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
                    if interpolation_depth == 0 && current == '\\' {
                        index += 1;
                        let Some((_, escaped)) = chars.get(index) else {
                            return Err(Diagnostic::at(
                                Span::new(line_no, column),
                                "unterminated f-string literal",
                            ));
                        };
                        let decoded = match escaped {
                            'n' => '\n',
                            't' => '\t',
                            '"' => '"',
                            '\\' => '\\',
                            other => {
                                return Err(Diagnostic::at(
                                    Span::new(line_no, column),
                                    format!("unsupported escape sequence `\\{}`", other),
                                ));
                            }
                        };
                        value.push(decoded);
                        index += 1;
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
                                '{' => interpolation_depth += 1,
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
                        index += 1;
                        let Some((_, escaped)) = chars.get(index) else {
                            return Err(Diagnostic::at(
                                Span::new(line_no, column),
                                "unterminated string literal",
                            ));
                        };
                        let decoded = match escaped {
                            'n' => '\n',
                            't' => '\t',
                            '"' => '"',
                            '\\' => '\\',
                            other => {
                                return Err(Diagnostic::at(
                                    Span::new(line_no, column),
                                    format!("unsupported escape sequence `\\{}`", other),
                                ));
                            }
                        };
                        value.push(decoded);
                        index += 1;
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

                let end_offset = chars
                    .get(index)
                    .map(|(next_offset, _)| *next_offset)
                    .unwrap_or_else(|| content.len());
                let text = &content[chars[start].0..end_offset];

                if is_float {
                    let value = text.parse::<f64>().map_err(|_| {
                        Diagnostic::at(Span::new(line_no, column), "invalid floating-point literal")
                    })?;
                    tokens.push(Token {
                        kind: TokenKind::FloatLiteral(value),
                        span: Span::new(line_no, column),
                    });
                } else {
                    let value = text.parse::<u128>().map_err(|_| {
                        Diagnostic::at(Span::new(line_no, column), "invalid integer literal")
                    })?;
                    let duration_kind = if let Some((_, suffix_start)) = chars.get(index) {
                        match suffix_start {
                            'm' => {
                                if matches!(chars.get(index + 1), Some((_, 's'))) {
                                    index += 2;
                                    Some(TokenKind::DurationLiteral(
                                        i128::try_from(value).map_err(|_| {
                                            Diagnostic::at(
                                                Span::new(line_no, column),
                                                "invalid duration literal",
                                            )
                                        })?,
                                    ))
                                } else {
                                    index += 1;
                                    Some(TokenKind::DurationLiteral(
                                        i128::try_from(value.checked_mul(60_000).ok_or_else(
                                            || {
                                                Diagnostic::at(
                                                    Span::new(line_no, column),
                                                    "invalid duration literal",
                                                )
                                            },
                                        )?)
                                        .map_err(
                                            |_| {
                                                Diagnostic::at(
                                                    Span::new(line_no, column),
                                                    "invalid duration literal",
                                                )
                                            },
                                        )?,
                                    ))
                                }
                            }
                            's' => {
                                index += 1;
                                Some(TokenKind::DurationLiteral(
                                    i128::try_from(value.checked_mul(1000).ok_or_else(|| {
                                        Diagnostic::at(
                                            Span::new(line_no, column),
                                            "invalid duration literal",
                                        )
                                    })?)
                                    .map_err(|_| {
                                        Diagnostic::at(
                                            Span::new(line_no, column),
                                            "invalid duration literal",
                                        )
                                    })?,
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
                    "select" => TokenKind::KwSelect,
                    "spawn" => TokenKind::KwSpawn,
                    "detached" => TokenKind::KwDetached,
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
