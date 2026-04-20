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
    pub render_path: Option<String>,
    pub render_source: Option<String>,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
            render_path: None,
            render_source: None,
        }
    }

    pub fn at(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
            render_path: None,
            render_source: None,
        }
    }

    pub fn with_render_context(
        mut self,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        self.render_path = Some(path.into());
        self.render_source = Some(source.into());
        self
    }

    pub fn render_with_source(&self, path: &str, source: &str) -> String {
        let (path, source) = match (&self.render_path, &self.render_source) {
            (Some(render_path), Some(render_source)) => {
                (render_path.as_str(), render_source.as_str())
            }
            _ => (path, source),
        };
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
#[path = "diag_tests.rs"]
mod tests;
