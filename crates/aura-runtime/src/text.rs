//! The bytes of a `str` value, with a flag that records whether they are
//! all ASCII.
//!
//! Where every byte is ASCII, each Unicode scalar is one byte, so the
//! scalar length, indexing, and slicing need no decoding. The flag is set
//! when the text is built and kept through every change, so no operation
//! scans the text to find it again.
//!
//! A text of at most [`INLINE_CAPACITY`] bytes keeps them in the value
//! itself, so making, copying, and freeing a short string, such as a word,
//! a field, or a slice, allocates nothing. A longer text keeps a `String`.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

/// The most bytes a text keeps in the value itself: what fits beside the
/// tag, the flag, and the length in the 32 bytes a `String` and its flag
/// take.
pub const INLINE_CAPACITY: usize = 29;

/// The text of a `str` value. It compares, orders, and hashes as its
/// bytes do.
pub struct Text(Repr);

enum Repr {
    Inline {
        ascii: bool,
        len: u8,
        bytes: [u8; INLINE_CAPACITY],
    },
    Heap {
        ascii: bool,
        text: String,
    },
}

impl Text {
    /// An empty text.
    pub const fn new() -> Self {
        Self(Repr::Inline {
            ascii: true,
            len: 0,
            bytes: [0; INLINE_CAPACITY],
        })
    }

    /// `text`, whose flag the caller already knows, in the value itself
    /// when it fits.
    fn from_str_with(text: &str, ascii: bool) -> Self {
        if text.len() <= INLINE_CAPACITY {
            let mut bytes = [0; INLINE_CAPACITY];
            bytes[..text.len()].copy_from_slice(text.as_bytes());
            Self(Repr::Inline {
                ascii,
                len: text.len() as u8,
                bytes,
            })
        } else {
            Self(Repr::Heap {
                ascii,
                text: text.to_owned(),
            })
        }
    }

    /// `left` followed by `right`, allocated once at its final size.
    pub fn concat(left: &Text, right: &Text) -> Text {
        let ascii = left.is_ascii() && right.is_ascii();
        let len = left.len() + right.len();
        if len <= INLINE_CAPACITY {
            let mut bytes = [0; INLINE_CAPACITY];
            bytes[..left.len()].copy_from_slice(left.as_bytes());
            bytes[left.len()..len].copy_from_slice(right.as_bytes());
            return Self(Repr::Inline {
                ascii,
                len: len as u8,
                bytes,
            });
        }
        let mut text = String::with_capacity(len);
        text.push_str(left);
        text.push_str(right);
        Self(Repr::Heap { ascii, text })
    }

    /// The text with each ASCII letter mapped by `map`, such as
    /// `u8::to_ascii_lowercase`, or `None` when the text is not ASCII.
    pub fn map_ascii(&self, map: fn(&u8) -> u8) -> Option<Text> {
        if !self.is_ascii() {
            return None;
        }
        if self.len() <= INLINE_CAPACITY {
            let mut bytes = [0; INLINE_CAPACITY];
            for (out, byte) in bytes.iter_mut().zip(self.as_bytes()) {
                *out = map(byte);
            }
            return Some(Self(Repr::Inline {
                ascii: true,
                len: self.len() as u8,
                bytes,
            }));
        }
        let bytes = self.as_bytes().iter().map(map).collect::<Vec<_>>();
        // SAFETY: mapping ASCII bytes to ASCII bytes keeps them UTF-8.
        let text = unsafe { String::from_utf8_unchecked(bytes) };
        Some(Self(Repr::Heap { ascii: true, text }))
    }

    /// Whether every byte is ASCII.
    pub fn is_ascii(&self) -> bool {
        match &self.0 {
            Repr::Inline { ascii, .. } | Repr::Heap { ascii, .. } => *ascii,
        }
    }

    /// The number of Unicode scalars.
    pub fn char_len(&self) -> usize {
        if self.is_ascii() {
            self.len()
        } else {
            self.chars().count()
        }
    }

    pub fn as_str(&self) -> &str {
        match &self.0 {
            Repr::Inline { len, bytes, .. } => {
                // SAFETY: the inline bytes are only ever filled from `str`s,
                // whole, so the first `len` are UTF-8.
                unsafe { std::str::from_utf8_unchecked(&bytes[..usize::from(*len)]) }
            }
            Repr::Heap { text, .. } => text,
        }
    }

    pub fn into_string(self) -> String {
        match self.0 {
            Repr::Heap { text, .. } => text,
            Repr::Inline { .. } => self.as_str().to_owned(),
        }
    }

    /// The bytes the text can hold before it allocates again.
    pub fn capacity(&self) -> usize {
        match &self.0 {
            Repr::Inline { .. } => INLINE_CAPACITY,
            Repr::Heap { text, .. } => text.capacity(),
        }
    }

    /// Moves an inline text, whose flag is `ascii`, to a `String` with room
    /// for `additional` more bytes.
    fn spill(
        &mut self,
        ascii: bool,
        additional: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        let mut text = String::new();
        text.try_reserve((self.len() + additional).max(2 * INLINE_CAPACITY))?;
        text.push_str(self.as_str());
        self.0 = Repr::Heap { ascii, text };
        Ok(())
    }

    /// Appends `other`, whose flag the caller knows.
    fn push_with(&mut self, other: &str, other_ascii: bool) {
        if let Repr::Inline { ascii, len, bytes } = &mut self.0 {
            let start = usize::from(*len);
            let end = start + other.len();
            if end <= INLINE_CAPACITY {
                bytes[start..end].copy_from_slice(other.as_bytes());
                *len = end as u8;
                *ascii &= other_ascii;
                return;
            }
        }
        self.reserve(other.len());
        if let Repr::Heap { ascii, text } = &mut self.0 {
            text.push_str(other);
            *ascii &= other_ascii;
        }
    }

    /// Appends `other`, keeping the flag.
    pub fn push_str(&mut self, other: &str) {
        self.push_with(other, other.is_ascii());
    }

    /// Appends text whose flag is already known.
    pub fn push_text(&mut self, other: &Text) {
        self.push_with(other.as_str(), other.is_ascii());
    }

    pub fn push(&mut self, ch: char) {
        let mut buffer = [0; 4];
        self.push_with(ch.encode_utf8(&mut buffer), ch.is_ascii());
    }

    /// The bytes from `range`, which must fall on scalar boundaries. A part
    /// of ASCII text is ASCII.
    pub fn slice(&self, range: std::ops::Range<usize>) -> Text {
        let part = &self.as_str()[range];
        Self::from_str_with(part, self.is_ascii() || part.is_ascii())
    }

    pub fn try_reserve(
        &mut self,
        additional: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        match &mut self.0 {
            Repr::Inline { len, .. } if usize::from(*len) + additional <= INLINE_CAPACITY => Ok(()),
            Repr::Inline { ascii, .. } => {
                let ascii = *ascii;
                self.spill(ascii, additional)
            }
            Repr::Heap { text, .. } => text.try_reserve(additional),
        }
    }

    pub fn reserve(&mut self, additional: usize) {
        let reserved = self.try_reserve(additional);
        reserved
            .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<u8>()));
    }

    pub fn clear(&mut self) {
        match &mut self.0 {
            Repr::Inline { ascii, len, .. } => {
                *len = 0;
                *ascii = true;
            }
            Repr::Heap { ascii, text } => {
                text.clear();
                *ascii = true;
            }
        }
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

/// A copy of a short text is kept inline, wherever the original lies.
impl Clone for Text {
    fn clone(&self) -> Self {
        match &self.0 {
            Repr::Inline { ascii, len, bytes } => Self(Repr::Inline {
                ascii: *ascii,
                len: *len,
                bytes: *bytes,
            }),
            Repr::Heap { ascii, text } => Self::from_str_with(text, *ascii),
        }
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self(Repr::Heap {
            ascii: text.is_ascii(),
            text,
        })
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Self::from_str_with(text, text.is_ascii())
    }
}

impl From<&String> for Text {
    fn from(text: &String) -> Self {
        Self::from(text.as_str())
    }
}

impl From<char> for Text {
    fn from(ch: char) -> Self {
        let mut buffer = [0; 4];
        Self::from_str_with(ch.encode_utf8(&mut buffer), ch.is_ascii())
    }
}

impl From<Text> for String {
    fn from(text: Text) -> Self {
        text.into_string()
    }
}

impl Deref for Text {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Text {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for Text {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl AsRef<std::path::Path> for Text {
    fn as_ref(&self) -> &std::path::Path {
        self.as_str().as_ref()
    }
}

impl AsRef<std::ffi::OsStr> for Text {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.as_str().as_ref()
    }
}

impl Borrow<str> for Text {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Text {}

impl PartialEq<str> for Text {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Text {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for Text {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl PartialOrd for Text {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Text {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for Text {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl FromIterator<char> for Text {
    fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<String>())
    }
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
