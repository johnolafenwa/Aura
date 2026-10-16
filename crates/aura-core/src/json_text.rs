//! A small JSON writer for the records the native runtime emits.
//!
//! Its output matches `serde_json`'s compact form byte for byte, so the
//! runtime can write these records without linking `serde_json`.

use std::fmt::Write as _;

/// Appends `text` as a JSON string literal.
pub fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Appends a JSON number.
pub fn push_number(out: &mut String, value: usize) {
    let _ = write!(out, "{value}");
}

/// Appends a JSON boolean.
pub fn push_bool(out: &mut String, value: bool) {
    out.push_str(if value { "true" } else { "false" });
}

/// Appends a JSON array whose items `push_item` writes.
pub fn push_array<T>(out: &mut String, items: &[T], mut push_item: impl FnMut(&mut String, &T)) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_item(out, item);
    }
    out.push(']');
}

/// Writes a JSON object one field at a time.
pub struct ObjectWriter<'a> {
    out: &'a mut String,
    first: bool,
}

impl<'a> ObjectWriter<'a> {
    pub fn new(out: &'a mut String) -> Self {
        out.push('{');
        Self { out, first: true }
    }

    /// Starts a field and returns the buffer its value goes in.
    pub fn field(&mut self, name: &str) -> &mut String {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        push_string(self.out, name);
        self.out.push(':');
        self.out
    }

    pub fn finish(self) {
        self.out.push('}');
    }
}
