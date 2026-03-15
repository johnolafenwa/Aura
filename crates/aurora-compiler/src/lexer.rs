use crate::diag::{Diagnostic, Result, Span};

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    Newline,
    Indent,
    Dedent,
    Eof,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Dot,
    Equal,
    Plus,
    Minus,
    Star,
    Slash,
    Arrow,
    KwClass,
    KwDef,
    KwMut,
    KwPublic,
    KwReturn,
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
            '=' => {
                tokens.push(simple(TokenKind::Equal, line_no, column));
                index += 1;
            }
            '+' => {
                tokens.push(simple(TokenKind::Plus, line_no, column));
                index += 1;
            }
            '*' => {
                tokens.push(simple(TokenKind::Star, line_no, column));
                index += 1;
            }
            '/' => {
                tokens.push(simple(TokenKind::Slash, line_no, column));
                index += 1;
            }
            '-' => {
                if let Some((_, '>')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::Arrow, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Minus, line_no, column));
                    index += 1;
                }
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
                    let value = text.parse::<i64>().map_err(|_| {
                        Diagnostic::at(Span::new(line_no, column), "invalid integer literal")
                    })?;
                    tokens.push(Token {
                        kind: TokenKind::IntLiteral(value),
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
                    "def" => TokenKind::KwDef,
                    "mut" => TokenKind::KwMut,
                    "public" => TokenKind::KwPublic,
                    "return" => TokenKind::KwReturn,
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
