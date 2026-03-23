use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
        }
    }

    pub fn at(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
        }
    }

    pub fn render_with_source(&self, path: &str, source: &str) -> String {
        match self.span {
            Some(span) => render_annotated(path, source, span, &self.message),
            None => format!("error: {}\n --> {}", self.message, path),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.span {
            write!(f, "{}: {}", span, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl Error for Diagnostic {}

pub type Result<T> = std::result::Result<T, Diagnostic>;

fn render_annotated(path: &str, source: &str, span: Span, message: &str) -> String {
    let location = format!("{}:{}:{}", path, span.line, span.column);
    let Some(line_text) = source.lines().nth(span.line.saturating_sub(1)) else {
        return format!("error: {}\n --> {}", message, location);
    };

    let line_number = span.line.to_string();
    let gutter_width = line_number.len();
    let safe_column = span.column.max(1);
    let caret_padding = " ".repeat(safe_column.saturating_sub(1));

    format!(
        "error: {message}\n --> {location}\n{blank:>width$} |\n{line_number:>width$} | {line_text}\n{blank:>width$} | {caret_padding}^",
        blank = "",
        width = gutter_width,
    )
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, Span};

    #[test]
    fn renders_annotated_diagnostics_with_source_context() {
        let diagnostic = Diagnostic::at(Span::new(2, 9), "unknown name `value`");
        let rendered =
            diagnostic.render_with_source("examples/demo.au", "def main():\n    print(value)\n");

        assert!(rendered.contains("error: unknown name `value`"));
        assert!(rendered.contains("--> examples/demo.au:2:9"));
        assert!(rendered.contains("2 |     print(value)"));
        assert!(rendered.contains("|         ^"));
    }
}
