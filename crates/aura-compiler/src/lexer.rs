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
    DoubleStar,
    DoubleStarEqual,
    Slash,
    SlashEqual,
    DoubleSlash,
    DoubleSlashEqual,
    Percent,
    PercentEqual,
    Ampersand,
    AmpersandEqual,
    Pipe,
    PipeEqual,
    Caret,
    CaretEqual,
    Tilde,
    ShiftLeft,
    ShiftLeftEqual,
    ShiftRight,
    ShiftRightEqual,
    Arrow,
    KwClass,
    KwEnum,
    KwDef,
    KwTrait,
    KwImpl,
    KwImport,
    KwFrom,
    KwMut,
    KwOwn,
    KwIndirect,
    KwPublic,
    KwExtern,
    KwOpaque,
    KwReturn,
    KwAssert,
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
    KwIs,
    KwWhile,
    KwBreak,
    KwContinue,
    KwPass,
    KwTry,
    KwWith,
    KwAs,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DelimiterKind {
    Parenthesis,
    Bracket,
    Brace,
}

impl DelimiterKind {
    fn opener(self) -> char {
        match self {
            Self::Parenthesis => '(',
            Self::Bracket => '[',
            Self::Brace => '{',
        }
    }

    fn closer(self) -> char {
        match self {
            Self::Parenthesis => ')',
            Self::Bracket => ']',
            Self::Brace => '}',
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct DelimiterFrame {
    kind: DelimiterKind,
    span: Span,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct PendingDelimitedMatch {
    container_depth: usize,
    base_indent: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LayoutIsland {
    /// Number of open delimiters containing this block-form match expression.
    container_depth: usize,
    /// Physical indentation of the `match` header. This is a local baseline,
    /// not an emitted `Indent`.
    base_indent: usize,
    /// Physical indentation levels whose `Indent` tokens were emitted inside
    /// the island. The baseline is retained as the first, non-emitting entry.
    indent_stack: Vec<usize>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LineLayout {
    Global,
    Island,
    Continuation,
}

#[derive(Default)]
struct LexState {
    delimiters: Vec<DelimiterFrame>,
    pending_delimited_matches: Vec<PendingDelimitedMatch>,
    layout_islands: Vec<LayoutIsland>,
}

impl LexState {
    fn prepare_line(
        &mut self,
        indent: usize,
        starts_with_closer: bool,
        span: Span,
        tokens: &mut Vec<Token>,
    ) -> Result<LineLayout> {
        loop {
            let Some(island) = self.layout_islands.last() else {
                return Ok(if self.delimiters.is_empty() {
                    LineLayout::Global
                } else {
                    LineLayout::Continuation
                });
            };

            let depth = self.delimiters.len();
            if depth < island.container_depth
                || (depth == island.container_depth
                    && (indent <= island.base_indent || starts_with_closer))
            {
                self.close_top_layout_island(span, tokens);
                continue;
            }

            if depth > island.container_depth {
                return Ok(LineLayout::Continuation);
            }

            update_indentation(
                indent,
                &mut self.layout_islands.last_mut().unwrap().indent_stack,
                span,
                tokens,
            )?;
            return Ok(LineLayout::Island);
        }
    }

    fn open_delimiter(&mut self, kind: DelimiterKind, span: Span) -> Result<()> {
        if self.delimiters.len() >= RECURSION_LIMIT {
            return Err(lexical_error(
                span,
                format!(
                    "delimiter nesting exceeds the supported recursion limit of {}",
                    RECURSION_LIMIT
                ),
            ));
        }
        self.delimiters.push(DelimiterFrame { kind, span });
        Ok(())
    }

    fn close_delimiter(
        &mut self,
        kind: DelimiterKind,
        span: Span,
        tokens: &mut Vec<Token>,
    ) -> Result<()> {
        let Some(frame) = self.delimiters.last().copied() else {
            return Err(lexical_error(
                span,
                format!(
                    "unexpected closing delimiter `{}` with no matching opener",
                    kind.closer()
                ),
            ));
        };

        if frame.kind != kind {
            return Err(lexical_error(
                span,
                format!(
                    "mismatched closing delimiter `{}`; expected `{}` to close `{}`",
                    kind.closer(),
                    frame.kind.closer(),
                    frame.kind.opener()
                ),
            )
            .with_secondary(
                frame.span,
                format!("opening delimiter `{}` is here", frame.kind.opener()),
            ));
        }

        let closing_depth = self.delimiters.len();
        while matches!(
            self.layout_islands.last(),
            Some(island) if island.container_depth == closing_depth
        ) {
            self.close_top_layout_island(span, tokens);
        }
        self.pending_delimited_matches
            .retain(|pending| pending.container_depth < closing_depth);
        self.delimiters.pop();
        Ok(())
    }

    fn note_match_keyword(&mut self, base_indent: usize) {
        if !self.delimiters.is_empty() {
            self.pending_delimited_matches.push(PendingDelimitedMatch {
                container_depth: self.delimiters.len(),
                base_indent,
            });
        }
    }

    fn note_colon(&mut self) {
        let depth = self.delimiters.len();
        let Some(index) = self
            .pending_delimited_matches
            .iter()
            .rposition(|pending| pending.container_depth == depth)
        else {
            return;
        };
        let pending = self.pending_delimited_matches.remove(index);
        self.layout_islands.push(LayoutIsland {
            container_depth: pending.container_depth,
            base_indent: pending.base_indent,
            indent_stack: vec![pending.base_indent],
        });
    }

    fn close_top_layout_island(&mut self, span: Span, tokens: &mut Vec<Token>) {
        let Some(mut island) = self.layout_islands.pop() else {
            return;
        };
        while island.indent_stack.len() > 1 {
            island.indent_stack.pop();
            tokens.push(Token {
                kind: TokenKind::Dedent,
                span,
            });
        }
    }

    fn should_emit_newline(&self) -> bool {
        match self.layout_islands.last() {
            Some(island) => self.delimiters.len() == island.container_depth,
            None => self.delimiters.is_empty(),
        }
    }

    fn unclosed_delimiter_error(&self, eof_span: Span) -> Option<Diagnostic> {
        self.delimiters.last().map(|frame| {
            lexical_error(
                eof_span,
                format!(
                    "unclosed delimiter `{}`; expected `{}` before end of file",
                    frame.kind.opener(),
                    frame.kind.closer()
                ),
            )
            .with_secondary(
                frame.span,
                format!("opening delimiter `{}` is here", frame.kind.opener()),
            )
        })
    }
}

fn update_indentation(
    indent: usize,
    indent_stack: &mut Vec<usize>,
    span: Span,
    tokens: &mut Vec<Token>,
) -> Result<()> {
    let current_indent = *indent_stack.last().unwrap();
    if indent > current_indent {
        indent_stack.push(indent);
        tokens.push(Token {
            kind: TokenKind::Indent,
            span,
        });
    } else if indent < current_indent {
        while indent < *indent_stack.last().unwrap() {
            indent_stack.pop();
            tokens.push(Token {
                kind: TokenKind::Dedent,
                span,
            });
        }
        if indent != *indent_stack.last().unwrap() {
            return Err(lexical_error(span, "inconsistent indentation"));
        }
    }
    Ok(())
}

fn decode_hex_digit(ch: char) -> Option<u32> {
    match ch {
        '0'..='9' => Some((ch as u32) - ('0' as u32)),
        'a'..='f' => Some((ch as u32) - ('a' as u32) + 10),
        'A'..='F' => Some((ch as u32) - ('A' as u32) + 10),
        _ => None,
    }
}

fn lexical_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::coded_at("AU1001", span, message)
}

fn duration_literal_nanos(value: u128, nanos_per_unit: u128, span: Span) -> Result<TokenKind> {
    let nanos = value
        .checked_mul(nanos_per_unit)
        .and_then(|value| i128::try_from(value).ok())
        .ok_or_else(|| lexical_error(span, "invalid duration literal"))?;
    Ok(TokenKind::DurationLiteral(nanos))
}

fn decode_escape(
    chars: &[(usize, char)],
    escape_start: usize,
    line_no: usize,
    column: usize,
    literal_kind: &str,
) -> Result<(char, usize)> {
    let Some((_, escaped)) = chars.get(escape_start) else {
        return Err(lexical_error(
            Span::new(line_no, column),
            format!("unterminated {} literal", literal_kind),
        ));
    };
    match escaped {
        'n' => Ok(('\n', escape_start + 1)),
        't' => Ok(('\t', escape_start + 1)),
        '"' => Ok(('"', escape_start + 1)),
        '\'' => Ok(('\'', escape_start + 1)),
        '\\' => Ok(('\\', escape_start + 1)),
        '0' => Ok(('\0', escape_start + 1)),
        'x' => {
            let Some((_, high)) = chars.get(escape_start + 1) else {
                return Err(lexical_error(
                    Span::new(line_no, column),
                    format!("unsupported escape sequence `\\{}`", escaped),
                ));
            };
            let Some((_, low)) = chars.get(escape_start + 2) else {
                return Err(lexical_error(
                    Span::new(line_no, column),
                    format!("unsupported escape sequence `\\{}`", escaped),
                ));
            };
            let Some(high) = decode_hex_digit(*high) else {
                return Err(lexical_error(
                    Span::new(line_no, column),
                    "invalid hexadecimal escape sequence",
                ));
            };
            let Some(low) = decode_hex_digit(*low) else {
                return Err(lexical_error(
                    Span::new(line_no, column),
                    "invalid hexadecimal escape sequence",
                ));
            };
            let value = ((high << 4) | low) as u8;
            Ok((char::from(value), escape_start + 3))
        }
        'u' => {
            if !matches!(chars.get(escape_start + 1), Some((_, '{'))) {
                return Err(lexical_error(
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
                        return Err(lexical_error(
                            Span::new(line_no, column),
                            "unicode escape sequences must include at least one hexadecimal digit",
                        ));
                    }
                    let Some(decoded) = char::from_u32(scalar) else {
                        return Err(lexical_error(
                            Span::new(line_no, column),
                            "unicode escape sequence is out of range",
                        ));
                    };
                    return Ok((decoded, index + 1));
                }
                let Some(digit) = decode_hex_digit(*candidate) else {
                    return Err(lexical_error(
                        Span::new(line_no, column),
                        "invalid unicode escape sequence",
                    ));
                };
                scalar = scalar
                    .checked_mul(16)
                    .and_then(|value| value.checked_add(digit))
                    .ok_or_else(|| {
                        lexical_error(
                            Span::new(line_no, column),
                            "unicode escape sequence is out of range",
                        )
                    })?;
                saw_digit = true;
                index += 1;
            }
            Err(lexical_error(
                Span::new(line_no, column),
                format!("unterminated {} literal", literal_kind),
            ))
        }
        other => Err(lexical_error(
            Span::new(line_no, column),
            format!("unsupported escape sequence `\\{}`", other),
        )),
    }
}

pub fn lex(source: &str) -> Result<Vec<Token>> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut tokens = Vec::new();
    let mut indent_stack = vec![0usize];
    let mut state = LexState::default();

    for (index, raw_line) in source.lines().enumerate() {
        let line_no = index + 1;

        if raw_line.contains('\t') {
            return Err(Diagnostic::coded_at(
                "AU1001",
                Span::new(line_no, 1),
                "tabs are not supported for indentation; use spaces",
            ));
        }

        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = raw_line.chars().take_while(|ch| *ch == ' ').count();
        let content = &raw_line[indent..];
        let line_span = Span::new(line_no, 1);

        let starts_with_closer = matches!(content.chars().next(), Some(')' | ']' | '}'));
        if state.prepare_line(indent, starts_with_closer, line_span, &mut tokens)?
            == LineLayout::Global
        {
            update_indentation(indent, &mut indent_stack, line_span, &mut tokens)?;
        }

        tokenize_line(
            content,
            line_no,
            indent + 1,
            indent,
            &mut state,
            &mut tokens,
        )?;
        if state.should_emit_newline() {
            tokens.push(Token {
                kind: TokenKind::Newline,
                span: Span::new(line_no, raw_line.len() + 1),
            });
        }
    }

    let line_count = source.lines().count();
    let eof_span = Span::new(line_count + 1, 1);
    let diagnostic_eof_span = if source.is_empty() || source.ends_with('\n') {
        eof_span
    } else {
        Span::new(
            line_count.max(1),
            source.rsplit('\n').next().map_or(1, |line| line.len() + 1),
        )
    };
    if let Some(error) = state.unclosed_delimiter_error(diagnostic_eof_span) {
        return Err(error);
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token {
            kind: TokenKind::Dedent,
            span: eof_span,
        });
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: eof_span,
    });

    Ok(tokens)
}

fn tokenize_line(
    content: &str,
    line_no: usize,
    base_column: usize,
    line_indent: usize,
    state: &mut LexState,
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
                state.open_delimiter(DelimiterKind::Parenthesis, Span::new(line_no, column))?;
                tokens.push(simple(TokenKind::LParen, line_no, column));
                index += 1;
            }
            ')' => {
                state.close_delimiter(
                    DelimiterKind::Parenthesis,
                    Span::new(line_no, column),
                    tokens,
                )?;
                tokens.push(simple(TokenKind::RParen, line_no, column));
                index += 1;
            }
            '[' => {
                state.open_delimiter(DelimiterKind::Bracket, Span::new(line_no, column))?;
                tokens.push(simple(TokenKind::LBracket, line_no, column));
                index += 1;
            }
            ']' => {
                state.close_delimiter(
                    DelimiterKind::Bracket,
                    Span::new(line_no, column),
                    tokens,
                )?;
                tokens.push(simple(TokenKind::RBracket, line_no, column));
                index += 1;
            }
            '{' => {
                state.open_delimiter(DelimiterKind::Brace, Span::new(line_no, column))?;
                tokens.push(simple(TokenKind::LBrace, line_no, column));
                index += 1;
            }
            '}' => {
                state.close_delimiter(DelimiterKind::Brace, Span::new(line_no, column), tokens)?;
                tokens.push(simple(TokenKind::RBrace, line_no, column));
                index += 1;
            }
            ':' => {
                tokens.push(simple(TokenKind::Colon, line_no, column));
                state.note_colon();
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
                    return Err(lexical_error(
                        Span::new(line_no, column),
                        "unexpected character `!`",
                    ));
                }
            }
            '<' => {
                if matches!(chars.get(index + 1), Some((_, '<')))
                    && matches!(chars.get(index + 2), Some((_, '=')))
                {
                    tokens.push(simple(TokenKind::ShiftLeftEqual, line_no, column));
                    index += 3;
                } else if matches!(chars.get(index + 1), Some((_, '<'))) {
                    tokens.push(simple(TokenKind::ShiftLeft, line_no, column));
                    index += 2;
                } else if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::LessEq, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Less, line_no, column));
                    index += 1;
                }
            }
            '>' => {
                if matches!(chars.get(index + 1), Some((_, '>')))
                    && matches!(chars.get(index + 2), Some((_, '=')))
                {
                    tokens.push(simple(TokenKind::ShiftRightEqual, line_no, column));
                    index += 3;
                } else if matches!(chars.get(index + 1), Some((_, '>'))) {
                    tokens.push(simple(TokenKind::ShiftRight, line_no, column));
                    index += 2;
                } else if let Some((_, '=')) = chars.get(index + 1) {
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
                if matches!(chars.get(index + 1), Some((_, '*')))
                    && matches!(chars.get(index + 2), Some((_, '=')))
                {
                    tokens.push(simple(TokenKind::DoubleStarEqual, line_no, column));
                    index += 3;
                } else if matches!(chars.get(index + 1), Some((_, '*'))) {
                    tokens.push(simple(TokenKind::DoubleStar, line_no, column));
                    index += 2;
                } else if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::StarEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Star, line_no, column));
                    index += 1;
                }
            }
            '/' => {
                if matches!(chars.get(index + 1), Some((_, '/')))
                    && matches!(chars.get(index + 2), Some((_, '=')))
                {
                    tokens.push(simple(TokenKind::DoubleSlashEqual, line_no, column));
                    index += 3;
                } else if matches!(chars.get(index + 1), Some((_, '/'))) {
                    tokens.push(simple(TokenKind::DoubleSlash, line_no, column));
                    index += 2;
                } else if let Some((_, '=')) = chars.get(index + 1) {
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
            '&' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::AmpersandEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Ampersand, line_no, column));
                    index += 1;
                }
            }
            '|' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::PipeEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Pipe, line_no, column));
                    index += 1;
                }
            }
            '^' => {
                if let Some((_, '=')) = chars.get(index + 1) {
                    tokens.push(simple(TokenKind::CaretEqual, line_no, column));
                    index += 2;
                } else {
                    tokens.push(simple(TokenKind::Caret, line_no, column));
                    index += 1;
                }
            }
            '~' => {
                tokens.push(simple(TokenKind::Tilde, line_no, column));
                index += 1;
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
            'f' if matches!(chars.get(index + 1), Some((_, '\''))) => {
                return Err(Diagnostic::coded_at(
                    "AU1002",
                    Span::new(line_no, column),
                    "f-strings must be double-quoted; use `f\"...\"`",
                ));
            }
            'f' if matches!(chars.get(index + 1), Some((_, '"'))) => {
                index += 2;
                let mut value = String::new();
                let mut interpolation_depth = 0usize;
                let mut interpolation_string_quote = None;
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
                        if let Some(quote) = interpolation_string_quote {
                            if interpolation_escape {
                                interpolation_escape = false;
                            } else if current == '\\' {
                                interpolation_escape = true;
                            } else if current == quote {
                                interpolation_string_quote = None;
                            }
                        } else {
                            match current {
                                '"' | '\'' => interpolation_string_quote = Some(current),
                                '{' => {
                                    if interpolation_depth >= RECURSION_LIMIT {
                                        return Err(lexical_error(
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
                    return Err(lexical_error(
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
            quote @ ('"' | '\'') => {
                index += 1;
                let mut value = String::new();

                while index < chars.len() {
                    let (_, current) = chars[index];
                    if current == quote {
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

                if !matches!(chars.get(index), Some((_, current)) if *current == quote) {
                    return Err(lexical_error(
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
                let prefixed_base = if ch == '0' {
                    match chars.get(index + 1).map(|(_, next)| *next) {
                        Some('x' | 'X') => Some((16, "hexadecimal")),
                        Some('b' | 'B') => Some((2, "binary")),
                        Some('o' | 'O') => Some((8, "octal")),
                        _ => None,
                    }
                } else {
                    None
                };

                if let Some((radix, base_name)) = prefixed_base {
                    let prefix = &content[chars[start].0
                        ..chars
                            .get(start + 2)
                            .map(|(offset, _)| *offset)
                            .unwrap_or(content.len())];
                    index += 2;
                    let digits_start = index;
                    while matches!(
                        chars.get(index),
                        Some((_, '0'..='9' | 'a'..='z' | 'A'..='Z' | '_'))
                    ) {
                        index += 1;
                    }
                    if index == digits_start {
                        return Err(lexical_error(
                            Span::new(line_no, column),
                            format!("integer literal requires digits after `{prefix}`"),
                        ));
                    }
                    for digit_index in digits_start..index {
                        let (_, digit) = chars[digit_index];
                        if digit == '_' {
                            let previous_is_digit = digit_index > digits_start
                                && chars[digit_index - 1].1.is_digit(radix);
                            let next_is_digit = chars
                                .get(digit_index + 1)
                                .is_some_and(|(_, next)| next.is_digit(radix));
                            if !previous_is_digit || !next_is_digit {
                                return Err(lexical_error(
                                    Span::new(line_no, base_column + chars[digit_index].0),
                                    "an underscore in an integer literal must appear between two digits valid for its base",
                                ));
                            }
                        } else if digit.to_digit(radix).is_none() {
                            return Err(lexical_error(
                                Span::new(line_no, base_column + chars[digit_index].0),
                                format!("invalid digit `{digit}` in {base_name} integer literal"),
                            ));
                        }
                    }
                    if matches!(chars.get(index), Some((_, '.')))
                        && matches!(chars.get(index + 1), Some((_, '0'..='9')))
                    {
                        return Err(lexical_error(
                            Span::new(line_no, column),
                            "base prefixes do not apply to floating-point literals",
                        ));
                    }
                    let end_offset = chars
                        .get(index)
                        .map(|(next_offset, _)| *next_offset)
                        .unwrap_or_else(|| content.len());
                    let digits = content[chars[digits_start].0..end_offset].replace('_', "");
                    let value = u128::from_str_radix(&digits, radix).map_err(|_| {
                        lexical_error(Span::new(line_no, column), "invalid integer literal")
                    })?;
                    tokens.push(Token {
                        kind: TokenKind::IntLiteral(value),
                        span: Span::new(line_no, column),
                    });
                    continue;
                }

                index += 1;
                while matches!(chars.get(index), Some((_, '0'..='9' | '_'))) {
                    index += 1;
                }
                let integer_end = index;
                let has_separator = chars[start..integer_end]
                    .iter()
                    .any(|(_, digit)| *digit == '_');
                for digit_index in start..integer_end {
                    if chars[digit_index].1 != '_' {
                        continue;
                    }
                    let between_digits = digit_index > start
                        && matches!(chars.get(digit_index - 1), Some((_, '0'..='9')))
                        && matches!(chars.get(digit_index + 1), Some((_, '0'..='9')));
                    if !between_digits {
                        return Err(lexical_error(
                            Span::new(line_no, base_column + chars[digit_index].0),
                            "an underscore in an integer literal must appear between two decimal digits",
                        ));
                    }
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
                    if matches!(chars.get(index), Some((_, '_'))) {
                        return Err(lexical_error(
                            Span::new(line_no, base_column + chars[index].0),
                            "integer separators do not apply to floating-point literals",
                        ));
                    }
                }

                if matches!(chars.get(index), Some((_, 'e' | 'E'))) {
                    let exponent_start = index;
                    let mut exponent_index = index + 1;
                    if matches!(chars.get(exponent_index), Some((_, '+' | '-'))) {
                        exponent_index += 1;
                    }
                    if !matches!(chars.get(exponent_index), Some((_, '0'..='9'))) {
                        return Err(lexical_error(
                            Span::new(line_no, base_column + chars[exponent_start].0),
                            "invalid floating-point literal",
                        ));
                    }
                    is_float = true;
                    index = exponent_index + 1;
                    while matches!(chars.get(index), Some((_, '0'..='9'))) {
                        index += 1;
                    }
                    if matches!(chars.get(index), Some((_, '_'))) {
                        return Err(lexical_error(
                            Span::new(line_no, base_column + chars[index].0),
                            "integer separators do not apply to floating-point literals",
                        ));
                    }
                }

                let end_offset = match chars.get(index) {
                    Some((next_offset, _)) => *next_offset,
                    None => content.len(),
                };
                let text = &content[chars[start].0..end_offset];

                if is_float {
                    if has_separator {
                        return Err(lexical_error(
                            Span::new(line_no, column),
                            "integer separators do not apply to floating-point literals",
                        ));
                    }
                    let value = text
                        .parse::<f64>()
                        .expect("lexer should only build syntactically valid float literals");
                    if !value.is_finite() {
                        return Err(lexical_error(
                            Span::new(line_no, column),
                            "floating-point literal is out of range",
                        ));
                    }
                    tokens.push(Token {
                        kind: TokenKind::FloatLiteral(value),
                        span: Span::new(line_no, column),
                    });
                } else {
                    let normalized = text.replace('_', "");
                    let value = match normalized.parse::<u128>() {
                        Ok(value) => value,
                        Err(_) => {
                            return Err(lexical_error(
                                Span::new(line_no, column),
                                "invalid integer literal",
                            ));
                        }
                    };
                    let duration_kind = if let Some((_, suffix_start)) = chars.get(index) {
                        match suffix_start {
                            'm' => {
                                if has_separator {
                                    return Err(lexical_error(
                                        Span::new(line_no, column),
                                        "integer separators do not apply to duration literals",
                                    ));
                                }
                                if matches!(chars.get(index + 1), Some((_, 's'))) {
                                    index += 2;
                                    Some(duration_literal_nanos(
                                        value,
                                        1_000_000,
                                        Span::new(line_no, column),
                                    )?)
                                } else {
                                    index += 1;
                                    Some(duration_literal_nanos(
                                        value,
                                        60_000_000_000,
                                        Span::new(line_no, column),
                                    )?)
                                }
                            }
                            's' => {
                                if has_separator {
                                    return Err(lexical_error(
                                        Span::new(line_no, column),
                                        "integer separators do not apply to duration literals",
                                    ));
                                }
                                index += 1;
                                Some(duration_literal_nanos(
                                    value,
                                    1_000_000_000,
                                    Span::new(line_no, column),
                                )?)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if duration_kind.is_none()
                        && matches!(chars.get(index), Some((_, '_' | 'a'..='z' | 'A'..='Z')))
                    {
                        return Err(lexical_error(
                            Span::new(line_no, base_column + chars[index].0),
                            "invalid integer literal",
                        ));
                    }
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
                    "own" => TokenKind::KwOwn,
                    "indirect" => TokenKind::KwIndirect,
                    "public" => TokenKind::KwPublic,
                    "extern" => TokenKind::KwExtern,
                    "opaque" => TokenKind::KwOpaque,
                    "return" => TokenKind::KwReturn,
                    "assert" => TokenKind::KwAssert,
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
                    "is" => TokenKind::KwIs,
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
                if kind == TokenKind::KwMatch {
                    state.note_match_keyword(line_indent);
                }
                tokens.push(Token {
                    kind,
                    span: Span::new(line_no, column),
                });
            }
            _ => {
                return Err(lexical_error(
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
