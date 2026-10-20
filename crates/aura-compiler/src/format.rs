//! Token-based source formatter for `aura fmt` and editor formatting.
//!
//! The formatter re-lays out tokens. It sets indentation, spacing, and blank
//! lines, and it joins or breaks bracketed lists at 88 columns. It never
//! rewrites the text of a token, so string, f-string, and triple-quoted
//! literals stay byte for byte. Comments stay on the line they started on.
//!
//! A result is accepted only when it lexes to the same token stream as the
//! input and parses. Otherwise formatting fails and the caller keeps the
//! original text.

use std::collections::HashSet;

use crate::diag::Diagnostic;
use crate::lexer::{lex, Token, TokenKind};

/// The column limit that reflow aims for.
pub const LINE_WIDTH: usize = 88;

const INDENT_WIDTH: usize = 4;

#[derive(Debug)]
pub enum FormatError {
    /// The input does not lex or parse, so it cannot be formatted.
    Syntax(Diagnostic),
    /// The formatted text would not reproduce the input token stream.
    Unstable(String),
}

/// Formats Aura source text.
///
/// The input must lex and parse once trailing spaces and tabs are removed.
/// The output ends with one newline unless it is empty. Line endings become
/// `\n` and a leading byte order mark is dropped.
pub fn format_source(source: &str) -> Result<String, FormatError> {
    let source = source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .replace("\r\n", "\n");
    let source = strip_trailing_whitespace(&source);
    let original = lex(&source).map_err(FormatError::Syntax)?;
    crate::parser::parse(&source).map_err(FormatError::Syntax)?;
    let formatted = Formatter::new(&source).format();
    verify(&original, &formatted)?;
    Ok(formatted)
}

/// Removes spaces and tabs at the end of each line that does not end inside
/// a multi-line string. The lexer rejects a stray tab, so this runs first.
fn strip_trailing_whitespace(source: &str) -> String {
    let starts_in_string = scan(source).starts_in_string;
    source
        .split('\n')
        .enumerate()
        .map(|(line, text)| {
            if starts_in_string[line + 1] {
                text
            } else {
                text.trim_end_matches([' ', '\t'])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Checks that `formatted` parses and lexes to the same tokens as `original`.
///
/// Spans are ignored. A comma directly before a closing bracket is ignored
/// too, unless it makes a one-element tuple, because reflow may add or keep
/// one without changing meaning. A comma between lambda parameters does not
/// separate tuple elements, so it does not make a later comma optional.
fn verify(original: &[Token], formatted: &str) -> Result<(), FormatError> {
    let unstable = |detail: String| {
        FormatError::Unstable(format!(
            "internal formatter error: the formatted text {detail}; the source was left unchanged"
        ))
    };
    let tokens =
        lex(formatted).map_err(|error| unstable(format!("does not lex ({})", error.message)))?;
    crate::parser::parse(formatted)
        .map_err(|error| unstable(format!("does not parse ({})", error.message)))?;
    let before = comparable_tokens(original);
    let after = comparable_tokens(&tokens);
    let mismatch = before
        .iter()
        .zip(&after)
        .position(|(left, right)| left.kind != right.kind)
        .or_else(|| (before.len() != after.len()).then_some(before.len().min(after.len())));
    match mismatch {
        None => Ok(()),
        Some(index) => {
            let line = after
                .get(index)
                .or(after.last())
                .map_or(1, |token| token.span.line);
            Err(unstable(format!(
                "changes the token stream at formatted line {line}"
            )))
        }
    }
}

/// One open bracket while the token guard walks a token stream.
#[derive(Default)]
struct GuardFrame {
    paren: bool,
    /// A comma at this level has separated two elements.
    separated: bool,
    /// A `lambda` at this level has not reached its `:`, so a comma here
    /// separates lambda parameters.
    lambda_parameters: bool,
}

fn comparable_tokens(tokens: &[Token]) -> Vec<&Token> {
    let mut frames: Vec<GuardFrame> = Vec::new();
    let mut kept = Vec::with_capacity(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        match &token.kind {
            TokenKind::LParen => frames.push(GuardFrame {
                paren: true,
                ..GuardFrame::default()
            }),
            TokenKind::LBracket | TokenKind::LBrace => frames.push(GuardFrame::default()),
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                frames.pop();
            }
            TokenKind::Identifier(name) if name == "lambda" => {
                let field = index > 0 && tokens[index - 1].kind == TokenKind::Dot;
                if let Some(frame) = frames.last_mut().filter(|_| !field) {
                    frame.lambda_parameters = true;
                }
            }
            TokenKind::Colon => {
                if let Some(frame) = frames.last_mut() {
                    frame.lambda_parameters = false;
                }
            }
            TokenKind::Comma => {
                let before_closer = matches!(
                    tokens.get(index + 1).map(|next| &next.kind),
                    Some(TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace)
                );
                if let Some(frame) = frames.last_mut().filter(|frame| !frame.lambda_parameters) {
                    let one_element_tuple = frame.paren && !frame.separated;
                    frame.separated = true;
                    if before_closer && !one_element_tuple {
                        continue;
                    }
                }
            }
            _ => {}
        }
        kept.push(token);
    }
    kept
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Name,
    Keyword,
    Literal,
    Open,
    Close,
    Comma,
    Colon,
    Dot,
    Operator,
}

#[derive(Debug)]
struct Tok {
    text: String,
    class: Class,
    /// Physical line, counted from zero, where the token starts.
    line: usize,
    /// Physical line where the token ends. It differs from `line` only for a
    /// triple-quoted string that spans lines.
    end_line: usize,
    /// Comment lines inside brackets that come directly before this token.
    leading: Vec<String>,
    /// A comment after this token on the same physical line.
    trailing: Option<String>,
}

/// A comment on its own line between statements.
#[derive(Debug)]
struct LineComment {
    line: usize,
    indent: usize,
    text: String,
}

const KEYWORDS: &[&str] = &[
    "class", "enum", "def", "trait", "impl", "import", "from", "mut", "own", "indirect", "public",
    "extern", "opaque", "return", "assert", "if", "elif", "else", "and", "or", "not", "match",
    "case", "for", "in", "while", "break", "continue", "pass", "try", "with", "as",
];

const OPERATORS: &[&str] = &[
    "**=", "//=", "<<=", ">>=", "**", "//", "<<", ">>", "<=", ">=", "==", "!=", "->", "+=", "-=",
    "*=", "/=", "%=", "&=", "|=", "^=",
];

const DECLARATION_KEYWORDS: &[&str] =
    &["class", "enum", "def", "trait", "impl", "extern", "opaque"];

struct Scanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
}

impl Scanner<'_> {
    fn at(&self, index: usize) -> u8 {
        self.bytes.get(index).copied().unwrap_or(0)
    }

    /// Returns the end offset and class of the token that starts at `start`.
    fn token(&self, start: usize) -> (usize, Class) {
        let byte = self.at(start);
        match byte {
            b'"' | b'\'' => (self.string_end(start), Class::Literal),
            b'f' if self.at(start + 1) == b'"' => (self.f_string_end(start + 1), Class::Literal),
            b'r' if matches!(self.at(start + 1), b'"' | b'\'') => {
                (self.raw_string_end(start + 1), Class::Literal)
            }
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let mut end = start + 1;
                while matches!(self.at(end), b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9') {
                    end += 1;
                }
                let text = &self.source[start..end];
                let class = if KEYWORDS.contains(&text) {
                    Class::Keyword
                } else if text == "true" || text == "false" {
                    Class::Literal
                } else {
                    Class::Name
                };
                (end, class)
            }
            b'0'..=b'9' => (self.number_end(start), Class::Literal),
            b'(' | b'[' | b'{' => (start + 1, Class::Open),
            b')' | b']' | b'}' => (start + 1, Class::Close),
            b',' => (start + 1, Class::Comma),
            b':' => (start + 1, Class::Colon),
            b'.' => (start + 1, Class::Dot),
            _ => {
                let rest = &self.source[start..];
                let length = OPERATORS
                    .iter()
                    .find(|operator| rest.starts_with(**operator))
                    .map_or_else(
                        || rest.chars().next().map_or(1, char::len_utf8),
                        |operator| operator.len(),
                    );
                (start + length, Class::Operator)
            }
        }
    }

    fn string_end(&self, start: usize) -> usize {
        let quote = self.at(start);
        if self.at(start + 1) == quote && self.at(start + 2) == quote {
            let mut index = start + 3;
            let mut backslashes = 0usize;
            while index < self.bytes.len() {
                let byte = self.at(index);
                if byte == quote
                    && backslashes.is_multiple_of(2)
                    && self.at(index + 1) == quote
                    && self.at(index + 2) == quote
                {
                    return index + 3;
                }
                backslashes = if byte == b'\\' { backslashes + 1 } else { 0 };
                index += 1;
            }
            return index;
        }
        let mut index = start + 1;
        while index < self.bytes.len() {
            match self.at(index) {
                b'\\' => index += 2,
                byte if byte == quote => return index + 1,
                _ => index += 1,
            }
        }
        index
    }

    fn raw_string_end(&self, quote_index: usize) -> usize {
        let quote = self.at(quote_index);
        let mut index = quote_index + 1;
        let mut backslashes = 0usize;
        while index < self.bytes.len() {
            let byte = self.at(index);
            if byte == quote && backslashes.is_multiple_of(2) {
                return index + 1;
            }
            backslashes = if byte == b'\\' { backslashes + 1 } else { 0 };
            index += 1;
        }
        index
    }

    /// Follows the lexer's f-string rules: escapes and doubled braces in the
    /// text, and nested string literals inside interpolations.
    fn f_string_end(&self, quote_index: usize) -> usize {
        let mut index = quote_index + 1;
        let mut depth = 0usize;
        let mut string_quote: Option<u8> = None;
        let mut triple_quote: Option<u8> = None;
        let mut escaped = false;
        while index < self.bytes.len() {
            let byte = self.at(index);
            let next = self.at(index + 1);
            if depth == 0 {
                match byte {
                    b'"' => return index + 1,
                    b'{' | b'}' if next == byte => index += 2,
                    b'\\' if next == b'u' && self.at(index + 2) == b'{' => {
                        while index < self.bytes.len() && self.at(index) != b'}' {
                            index += 1;
                        }
                        index += 1;
                    }
                    b'\\' => index += 2,
                    b'{' => {
                        depth = 1;
                        index += 1;
                    }
                    _ => index += 1,
                }
                continue;
            }
            if let Some(quote) = triple_quote {
                if byte == quote && next == quote && self.at(index + 2) == quote {
                    triple_quote = None;
                    index += 3;
                    continue;
                }
            } else if let Some(quote) = string_quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == quote {
                    string_quote = None;
                }
            } else {
                match byte {
                    b'"' | b'\'' if next == byte && self.at(index + 2) == byte => {
                        triple_quote = Some(byte);
                        index += 3;
                        continue;
                    }
                    b'"' | b'\'' => string_quote = Some(byte),
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
            }
            index += 1;
        }
        index
    }

    fn number_end(&self, start: usize) -> usize {
        let is_digit = |byte: u8| byte.is_ascii_digit();
        if self.at(start) == b'0'
            && matches!(self.at(start + 1), b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
        {
            let mut index = start + 2;
            while self.at(index).is_ascii_alphanumeric() || self.at(index) == b'_' {
                index += 1;
            }
            return index;
        }
        let mut index = start;
        while is_digit(self.at(index)) || self.at(index) == b'_' {
            index += 1;
        }
        let mut float = false;
        if self.at(index) == b'.' && is_digit(self.at(index + 1)) {
            float = true;
            index += 1;
            while is_digit(self.at(index)) {
                index += 1;
            }
        }
        if matches!(self.at(index), b'e' | b'E') {
            let signed = matches!(self.at(index + 1), b'+' | b'-');
            let digit_at = index + 1 + usize::from(signed);
            if is_digit(self.at(digit_at)) {
                float = true;
                index = digit_at;
                while is_digit(self.at(index)) {
                    index += 1;
                }
            }
        }
        if !float {
            if self.at(index) == b'm' && self.at(index + 1) == b's' {
                index += 2;
            } else if matches!(self.at(index), b'm' | b's') {
                index += 1;
            }
        }
        index
    }
}

struct Scan {
    toks: Vec<Tok>,
    comments: Vec<LineComment>,
    /// For each physical line, whether it begins inside a multi-line string.
    starts_in_string: Vec<bool>,
}

fn scan(source: &str) -> Scan {
    let scanner = Scanner {
        source,
        bytes: source.as_bytes(),
    };
    let line_count = source.split('\n').count();
    let mut toks: Vec<Tok> = Vec::new();
    let mut comments = Vec::new();
    let mut starts_in_string = vec![false; line_count + 1];
    let mut pending: Vec<String> = Vec::new();
    let mut index = 0usize;
    let mut line = 0usize;
    let mut line_start = 0usize;
    let mut depth = 0usize;
    while index < source.len() {
        match scanner.at(index) {
            b'\n' => {
                line += 1;
                index += 1;
                line_start = index;
            }
            b' ' | b'\t' | b'\r' => index += 1,
            b'#' => {
                let end = source[index..]
                    .find('\n')
                    .map_or(source.len(), |offset| index + offset);
                let text = source[index..end].trim_end().to_string();
                match toks.last_mut().filter(|tok| tok.end_line == line) {
                    Some(tok) => tok.trailing = Some(text),
                    None if depth > 0 => pending.push(text),
                    None => comments.push(LineComment {
                        line,
                        indent: index - line_start,
                        text,
                    }),
                }
                index = end;
            }
            _ => {
                let (end, class) = scanner.token(index);
                let end = end.min(source.len());
                let text = &source[index..end];
                let newlines = text.matches('\n').count();
                for flag in &mut starts_in_string[line + 1..=line + newlines] {
                    *flag = true;
                }
                match class {
                    Class::Open => depth += 1,
                    Class::Close => depth = depth.saturating_sub(1),
                    _ => {}
                }
                toks.push(Tok {
                    text: text.to_string(),
                    class,
                    line,
                    end_line: line + newlines,
                    leading: std::mem::take(&mut pending),
                    trailing: None,
                });
                if newlines > 0 {
                    line += newlines;
                    line_start = index + text.rfind('\n').map_or(0, |offset| offset + 1);
                }
                index = end;
            }
        }
    }
    Scan {
        toks,
        comments,
        starts_in_string,
    }
}

/// One statement line: the tokens from one depth-zero newline to the next.
#[derive(Clone, Copy, Debug)]
struct Logical {
    first: usize,
    last: usize,
}

#[derive(Clone, Copy, Debug)]
enum Entry {
    Line(Logical),
    Comment(usize),
}

#[derive(Debug)]
enum Item {
    Tok(usize),
    Group(Group),
}

#[derive(Debug)]
struct Group {
    open: usize,
    close: usize,
    elements: Vec<Element>,
    /// The group must be written one element per line.
    forced: bool,
    /// An exploded layout ends with a trailing comma. A list of two or more
    /// elements takes one.
    takes_trailing_comma: bool,
}

#[derive(Debug)]
struct Element {
    items: Vec<Item>,
    comma: Option<usize>,
}

/// One output line before it becomes text.
enum Out {
    Code(usize, Vec<usize>),
    Comment(usize, String),
}

#[derive(Clone, Copy)]
struct BracketContext {
    paren: bool,
    slice: bool,
    /// The current element has a type annotation, so its default is spaced.
    annotated: bool,
}

struct Formatter<'a> {
    lines: Vec<&'a str>,
    toks: Vec<Tok>,
    comments: Vec<LineComment>,
    starts_in_string: Vec<bool>,
    space_before: Vec<bool>,
    synthetic_comma: usize,
}

impl<'a> Formatter<'a> {
    fn new(source: &'a str) -> Self {
        let Scan {
            mut toks,
            comments,
            starts_in_string,
        } = scan(source);
        let synthetic_comma = toks.len();
        toks.push(Tok {
            text: ",".to_string(),
            class: Class::Comma,
            line: usize::MAX,
            end_line: usize::MAX,
            leading: Vec::new(),
            trailing: None,
        });
        let space_before = vec![false; toks.len()];
        Self {
            lines: source.split('\n').collect(),
            toks,
            comments,
            starts_in_string,
            space_before,
            synthetic_comma,
        }
    }

    fn format(mut self) -> String {
        let logical = self.logical_lines();
        for line in &logical {
            self.compute_spacing(*line);
        }
        let entries = self.entries(&logical);
        let mut output: Vec<String> = Vec::new();
        let mut stack = vec![0usize];
        // A block header was the last statement, so its body has not started.
        let mut body_pending = false;
        let mut first_in_body = false;
        let mut in_declaration = false;
        let mut previous_end: Option<usize> = None;
        let mut previous_top_comment = false;
        // A decorator line holds its comments and definition with no blank
        // line between them.
        let mut after_decorator = false;
        for (position, entry) in entries.iter().enumerate() {
            let (start, end) = self.entry_lines(*entry);
            let original_blanks =
                previous_end.map_or(0, |previous| start.saturating_sub(previous + 1));
            let level = match *entry {
                Entry::Line(line) => {
                    let indent = self.original_indent(self.toks[line.first].line);
                    while stack.len() > 1 && indent < *stack.last().unwrap_or(&0) {
                        stack.pop();
                    }
                    if indent > *stack.last().unwrap_or(&0) {
                        stack.push(indent);
                    }
                    stack.len() - 1
                }
                Entry::Comment(index) => {
                    let indent = self.comments[index].indent;
                    if body_pending && indent > *stack.last().unwrap_or(&0) {
                        stack.len()
                    } else {
                        // A comment indented past a block's header belongs to
                        // the block.
                        stack
                            .iter()
                            .position(|level| *level >= indent)
                            .unwrap_or(stack.len() - 1)
                    }
                }
            };
            let blanks = if output.is_empty() || first_in_body || after_decorator {
                0
            } else if level > 0 {
                original_blanks.min(1)
            } else if original_blanks == 0 && previous_top_comment {
                0
            } else if in_declaration || self.starts_declaration_cluster(&entries[position..]) {
                2
            } else {
                original_blanks.min(2)
            };
            output.extend(std::iter::repeat_n(String::new(), blanks));
            match *entry {
                Entry::Line(line) => {
                    output.extend(self.render_logical(line, level));
                    body_pending = self.toks[line.last].class == Class::Colon;
                    first_in_body = body_pending;
                    if level == 0 {
                        in_declaration = self.is_declaration(line);
                    }
                    after_decorator = self.is_decorator(line);
                    previous_top_comment = false;
                }
                Entry::Comment(index) => {
                    output.push(format!(
                        "{}{}",
                        " ".repeat(level * INDENT_WIDTH),
                        self.comments[index].text
                    ));
                    previous_top_comment = level == 0;
                    first_in_body = false;
                }
            }
            previous_end = Some(end);
        }
        if output.is_empty() {
            return String::new();
        }
        let mut text = output.join("\n");
        text.push('\n');
        text
    }

    fn logical_lines(&self) -> Vec<Logical> {
        let mut lines = Vec::new();
        let mut depth = 0usize;
        let real = self.synthetic_comma;
        let mut first = 0usize;
        for index in 0..real {
            if index > first && depth == 0 && self.toks[index].line > self.toks[index - 1].end_line
            {
                lines.push(Logical {
                    first,
                    last: index - 1,
                });
                first = index;
            }
            match self.toks[index].class {
                Class::Open => depth += 1,
                Class::Close => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if real > 0 {
            lines.push(Logical {
                first,
                last: real - 1,
            });
        }
        lines
    }

    fn entries(&self, logical: &[Logical]) -> Vec<Entry> {
        let mut entries = Vec::with_capacity(logical.len() + self.comments.len());
        let mut comments = (0..self.comments.len()).peekable();
        for line in logical {
            let start = self.toks[line.first].line;
            while let Some(index) = comments.next_if(|index| self.comments[*index].line < start) {
                entries.push(Entry::Comment(index));
            }
            entries.push(Entry::Line(*line));
        }
        entries.extend(comments.map(Entry::Comment));
        entries
    }

    fn entry_lines(&self, entry: Entry) -> (usize, usize) {
        match entry {
            Entry::Line(line) => (self.toks[line.first].line, self.toks[line.last].end_line),
            Entry::Comment(index) => (self.comments[index].line, self.comments[index].line),
        }
    }

    fn original_indent(&self, line: usize) -> usize {
        self.lines[line]
            .bytes()
            .take_while(|byte| *byte == b' ')
            .count()
    }

    /// Whether this line is an `@` decorator line.
    fn is_decorator(&self, line: Logical) -> bool {
        self.toks[line.first].text == "@"
    }

    fn is_declaration(&self, line: Logical) -> bool {
        if self.is_decorator(line) {
            return true;
        }
        let mut index = line.first;
        if self.toks[index].text == "public" && index < line.last {
            index += 1;
        }
        let tok = &self.toks[index];
        tok.class == Class::Keyword && DECLARATION_KEYWORDS.contains(&tok.text.as_str())
    }

    /// Whether these entries open with a top-level declaration, or with
    /// comments attached to one by having no blank line between them.
    fn starts_declaration_cluster(&self, entries: &[Entry]) -> bool {
        let mut previous_end: Option<usize> = None;
        for entry in entries {
            let (start, end) = self.entry_lines(*entry);
            if previous_end.is_some_and(|previous| start != previous + 1) {
                return false;
            }
            match *entry {
                Entry::Line(line) => {
                    return self.original_indent(start) == 0 && self.is_declaration(line);
                }
                Entry::Comment(index) if self.comments[index].indent == 0 => {}
                Entry::Comment(_) => return false,
            }
            previous_end = Some(end);
        }
        false
    }

    fn is_name_like(&self, index: usize, first: usize) -> bool {
        let tok = &self.toks[index];
        match tok.class {
            Class::Name | Class::Literal => true,
            Class::Keyword => {
                index > first && matches!(self.toks[index - 1].text.as_str(), "." | "def")
            }
            _ => false,
        }
    }

    fn ends_value(&self, index: usize, first: usize) -> bool {
        self.toks[index].class == Class::Close || self.is_name_like(index, first)
    }

    fn is_unary(&self, index: usize, first: usize) -> bool {
        if !matches!(self.toks[index].text.as_str(), "-" | "+" | "~" | "*" | "**") {
            return false;
        }
        index == first || {
            let previous = &self.toks[index - 1];
            match previous.class {
                Class::Open | Class::Comma | Class::Colon | Class::Operator => true,
                Class::Keyword => !self.is_name_like(index - 1, first),
                _ => false,
            }
        }
    }

    /// Whether the `[` at `index` indexes or slices a value, as opposed to a
    /// list display, a type-parameter list, or a lambda capture list.
    fn opens_subscript(&self, index: usize, first: usize) -> bool {
        if index == first {
            return false;
        }
        let previous = index - 1;
        self.ends_value(previous, first)
            && self.toks[previous].text != "lambda"
            && !(previous > first
                && matches!(
                    self.toks[previous - 1].text.as_str(),
                    "def" | "class" | "enum" | "trait"
                ))
    }

    fn compute_spacing(&mut self, line: Logical) {
        let mut stack: Vec<BracketContext> = Vec::new();
        for index in line.first..=line.last {
            let space = index != line.first && self.space_between(index, line.first, stack.last());
            self.space_before[index] = space;
            let tok = &self.toks[index];
            match tok.class {
                Class::Open => {
                    let slice = tok.text == "[" && self.opens_subscript(index, line.first);
                    stack.push(BracketContext {
                        paren: tok.text == "(",
                        slice,
                        annotated: false,
                    });
                }
                Class::Close => {
                    stack.pop();
                }
                Class::Comma | Class::Colon => {
                    if let Some(context) = stack.last_mut() {
                        context.annotated = tok.class == Class::Colon;
                    }
                }
                _ => {}
            }
        }
    }

    fn space_between(&self, index: usize, first: usize, context: Option<&BracketContext>) -> bool {
        let left = &self.toks[index - 1];
        let right = &self.toks[index];
        if left.class == Class::Open
            || matches!(right.class, Class::Close | Class::Comma | Class::Colon)
        {
            return false;
        }
        if left.class == Class::Comma {
            return true;
        }
        if left.class == Class::Colon {
            return !context.is_some_and(|context| context.slice);
        }
        if left.class == Class::Dot || right.class == Class::Dot {
            return false;
        }
        if index - 1 == first && left.text == "@" {
            return false;
        }
        if self.is_unary(index - 1, first) {
            return false;
        }
        if left.text == "=" || right.text == "=" {
            return !context.is_some_and(|context| context.paren && !context.annotated);
        }
        if right.class == Class::Open && right.text != "{" {
            let attached = (self.ends_value(index - 1, first) && left.text != "lambda")
                || matches!(left.text.as_str(), "def" | "impl");
            return !attached;
        }
        true
    }

    fn has_layout_island(&self, line: Logical) -> bool {
        let mut depth = 0usize;
        for tok in &self.toks[line.first..=line.last] {
            match tok.class {
                Class::Open => depth += 1,
                Class::Close => depth = depth.saturating_sub(1),
                Class::Keyword if depth > 0 && tok.text == "match" => return true,
                _ => {}
            }
        }
        false
    }

    fn render_logical(&self, line: Logical, level: usize) -> Vec<String> {
        if self.has_layout_island(line) {
            return self.verbatim(line, level * INDENT_WIDTH);
        }
        let mut cursor = line.first;
        let mut items = Vec::new();
        while cursor <= line.last {
            items.push(self.build_item(&mut cursor));
        }
        let mut out = Vec::new();
        let mut emitted = HashSet::new();
        self.layout(None, &items, None, level, &mut out, &mut emitted);
        self.render(&out, &emitted)
            .unwrap_or_else(|| self.verbatim(line, level * INDENT_WIDTH))
    }

    fn build_item(&self, cursor: &mut usize) -> Item {
        let index = *cursor;
        if self.toks[index].class != Class::Open {
            *cursor += 1;
            return Item::Tok(index);
        }
        let open = index;
        *cursor += 1;
        let mut elements = Vec::new();
        let mut current = Vec::new();
        // A comma between a `lambda` and its `:` separates the lambda's
        // parameters, not the elements of this list.
        let mut lambda_parameters = false;
        while *cursor < self.synthetic_comma && self.toks[*cursor].class != Class::Close {
            let tok = &self.toks[*cursor];
            match tok.class {
                Class::Comma if !lambda_parameters => {
                    elements.push(Element {
                        items: std::mem::take(&mut current),
                        comma: Some(*cursor),
                    });
                    *cursor += 1;
                    continue;
                }
                Class::Colon => lambda_parameters = false,
                Class::Name if tok.text == "lambda" => {
                    lambda_parameters = self.toks[*cursor - 1].class != Class::Dot;
                }
                _ => {}
            }
            current.push(self.build_item(cursor));
        }
        if !current.is_empty() {
            elements.push(Element {
                items: current,
                comma: None,
            });
        }
        // A comprehension's commas separate its targets, not list elements,
        // so it lays out as one element with no trailing comma.
        let comprehension = elements
            .iter()
            .flat_map(|element| &element.items)
            .any(|item| matches!(item, Item::Tok(index) if self.toks[*index].text == "for"));
        if comprehension && elements.len() > 1 {
            let mut items = Vec::new();
            for element in elements.drain(..) {
                items.extend(element.items);
                items.extend(element.comma.map(Item::Tok));
            }
            elements.push(Element { items, comma: None });
        }
        let close = *cursor;
        *cursor += 1;
        let magic = elements
            .last()
            .is_some_and(|element| element.comma.is_some())
            && !(self.toks[open].text == "(" && elements.len() == 1);
        let commented = self.toks[open].trailing.is_some()
            || self.toks[open + 1..close]
                .iter()
                .any(|tok| tok.trailing.is_some() || !tok.leading.is_empty())
            || !self.toks[close].leading.is_empty();
        let nested_forced = elements
            .iter()
            .flat_map(|element| &element.items)
            .any(|item| matches!(item, Item::Group(group) if group.forced));
        let takes_trailing_comma = elements.len() > 1;
        Item::Group(Group {
            open,
            close,
            elements,
            forced: magic || commented || nested_forced,
            takes_trailing_comma,
        })
    }

    fn layout(
        &self,
        prefix: Option<usize>,
        items: &[Item],
        suffix: Option<usize>,
        indent: usize,
        out: &mut Vec<Out>,
        emitted: &mut HashSet<usize>,
    ) {
        let mut flat = Vec::new();
        flat.extend(prefix);
        flatten(items, &mut flat);
        flat.extend(suffix);
        let forced = items
            .iter()
            .any(|item| matches!(item, Item::Group(group) if group.forced));
        if !forced && self.fits(&flat, indent) {
            out.push(Out::Code(indent, flat));
            return;
        }
        let Some((split, group)) = self.choose_split(prefix, items, suffix, indent) else {
            out.push(Out::Code(indent, flat));
            return;
        };
        let mut head = Vec::new();
        head.extend(prefix);
        flatten(&items[..split], &mut head);
        head.push(group.open);
        out.push(Out::Code(indent, head));
        let count = group.elements.len();
        for (position, element) in group.elements.iter().enumerate() {
            if let Some(first) = first_token(&element.items) {
                self.emit_leading(first, indent + 1, out, emitted);
            }
            let comma = element.comma.or_else(|| {
                (position + 1 == count && group.takes_trailing_comma)
                    .then_some(self.synthetic_comma)
            });
            self.layout(None, &element.items, comma, indent + 1, out, emitted);
        }
        self.emit_leading(group.close, indent + 1, out, emitted);
        self.layout(
            Some(group.close),
            &items[split + 1..],
            suffix,
            indent,
            out,
            emitted,
        );
    }

    /// Picks the bracket to break. A group that must break wins. Otherwise
    /// the rightmost group whose opening and closing lines both fit, where a
    /// group after `->` comes last so a signature breaks its parameters
    /// before its result type. Otherwise the first group.
    fn choose_split<'g>(
        &self,
        prefix: Option<usize>,
        items: &'g [Item],
        suffix: Option<usize>,
        indent: usize,
    ) -> Option<(usize, &'g Group)> {
        let mut candidates: Vec<(usize, &Group)> = items
            .iter()
            .enumerate()
            .filter_map(|(position, item)| match item {
                Item::Group(group) if group.forced || !group.elements.is_empty() => {
                    Some((position, group))
                }
                _ => None,
            })
            .collect();
        if let Some(forced) = candidates.iter().find(|(_, group)| group.forced) {
            return Some(*forced);
        }
        let fallback = candidates.first().copied();
        let signature = prefix.is_none()
            && matches!(
                first_token(items).map(|index| self.toks[index].text.as_str()),
                Some("def" | "public" | "extern")
            );
        let arrow = items.iter().position(|item| {
            signature && matches!(item, Item::Tok(index) if self.toks[*index].text == "->")
        });
        candidates.reverse();
        candidates.sort_by_key(|(position, _)| arrow.is_some_and(|arrow| *position > arrow));
        candidates
            .into_iter()
            .find(|(position, group)| {
                let mut head = Vec::new();
                head.extend(prefix);
                flatten(&items[..*position], &mut head);
                head.push(group.open);
                let mut tail = vec![group.close];
                flatten(&items[position + 1..], &mut tail);
                tail.extend(suffix);
                self.fits(&head, indent) && self.fits(&tail, indent)
            })
            .or(fallback)
    }

    fn emit_leading(
        &self,
        index: usize,
        indent: usize,
        out: &mut Vec<Out>,
        emitted: &mut HashSet<usize>,
    ) {
        for comment in &self.toks[index].leading {
            out.push(Out::Comment(indent, comment.clone()));
        }
        emitted.insert(index);
    }

    /// Whether the tokens fit on one line at this indent. Only the first and
    /// last physical line of a multi-line string count, because its inner
    /// lines are literal content.
    fn fits(&self, tokens: &[usize], indent: usize) -> bool {
        let mut column = indent * INDENT_WIDTH;
        for (position, index) in tokens.iter().enumerate() {
            if position > 0 && self.space_before[*index] {
                column += 1;
            }
            let text = &self.toks[*index].text;
            match (text.find('\n'), text.rfind('\n')) {
                (Some(first_break), Some(last_break)) => {
                    if column + text[..first_break].chars().count() > LINE_WIDTH {
                        return false;
                    }
                    column = text[last_break + 1..].chars().count();
                }
                _ => column += text.chars().count(),
            }
        }
        column <= LINE_WIDTH
    }

    fn join(&self, tokens: &[usize]) -> String {
        let mut text = String::new();
        for (position, index) in tokens.iter().enumerate() {
            if position > 0 && self.space_before[*index] {
                text.push(' ');
            }
            text.push_str(&self.toks[*index].text);
        }
        text
    }

    /// Turns layout into text. Returns `None` when a comment cannot stay at
    /// the end of its line or before its token, so the caller keeps the
    /// original layout instead.
    fn render(&self, out: &[Out], emitted: &HashSet<usize>) -> Option<Vec<String>> {
        let mut lines = Vec::with_capacity(out.len());
        for line in out {
            match line {
                Out::Comment(indent, text) => {
                    lines.push(format!("{}{text}", " ".repeat(indent * INDENT_WIDTH)));
                }
                Out::Code(indent, tokens) => {
                    for (position, index) in tokens.iter().enumerate() {
                        let tok = &self.toks[*index];
                        let leading_ok =
                            tok.leading.is_empty() || (position == 0 && emitted.contains(index));
                        // An added trailing comma goes before the comment.
                        let last = if tokens.last() == Some(&self.synthetic_comma) {
                            tokens.len().saturating_sub(2)
                        } else {
                            tokens.len() - 1
                        };
                        let trailing_ok = tok.trailing.is_none() || position == last;
                        if !leading_ok || !trailing_ok {
                            return None;
                        }
                    }
                    let mut text = " ".repeat(indent * INDENT_WIDTH);
                    text.push_str(&self.join(tokens));
                    if let Some(comment) = tokens
                        .iter()
                        .rev()
                        .take(2)
                        .find_map(|index| self.toks[*index].trailing.as_ref())
                    {
                        text.push_str("  ");
                        text.push_str(comment);
                    }
                    lines.push(text);
                }
            }
        }
        Some(lines)
    }

    /// Keeps the original physical lines, shifted to the new indentation.
    /// Lines that start inside a string are copied unchanged.
    fn verbatim(&self, line: Logical, indent: usize) -> Vec<String> {
        let start = self.toks[line.first].line;
        let end = self.toks[line.last].end_line;
        let original = self.original_indent(start);
        (start..=end)
            .map(|physical| {
                let raw = self.lines[physical];
                let mut text = if self.starts_in_string[physical] {
                    raw.to_string()
                } else {
                    let spaces = self.original_indent(physical);
                    let content = &raw[spaces..];
                    if content.is_empty() {
                        String::new()
                    } else {
                        let shifted = (spaces + indent).saturating_sub(original);
                        format!("{}{content}", " ".repeat(shifted))
                    }
                };
                if !self.starts_in_string[physical + 1] {
                    text.truncate(text.trim_end().len());
                }
                text
            })
            .collect()
    }
}

fn flatten(items: &[Item], tokens: &mut Vec<usize>) {
    for item in items {
        match item {
            Item::Tok(index) => tokens.push(*index),
            Item::Group(group) => {
                tokens.push(group.open);
                for element in &group.elements {
                    flatten(&element.items, tokens);
                    tokens.extend(element.comma);
                }
                tokens.push(group.close);
            }
        }
    }
}

fn first_token(items: &[Item]) -> Option<usize> {
    items.first().map(|item| match item {
        Item::Tok(index) => *index,
        Item::Group(group) => group.open,
    })
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
