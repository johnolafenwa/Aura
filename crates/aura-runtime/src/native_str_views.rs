//! Text entry points that read their input where it lies.
//!
//! Each takes a `str` as the address and length of its UTF-8 bytes, and
//! none takes or makes a box: generated code that holds a literal, an
//! element of a `list[str]` view, or a `str` kept as words calls them
//! without first building a value. A result that is part of the input comes
//! back as an address and a length inside it, so the caller keeps the input
//! alive for as long as it reads the result.
//!
//! The entry points that only read cannot fail, so they do not cross the
//! runtime boundary. A [`DirectStrBuffer`] can pass the string limit, which
//! stops the program as it does for any other text.

use std::slice;

/// The text at `ptr`.
///
/// # Safety
///
/// `ptr` addresses `len` bytes of UTF-8 that stay unchanged while the
/// result is read. A `len` of zero reads nothing, whatever `ptr` is.
#[inline(always)]
unsafe fn view<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 {
        return "";
    }
    // SAFETY: the caller's contract.
    unsafe { str::from_utf8_unchecked(slice::from_raw_parts(ptr, len)) }
}

/// Where `part` lies in `text`, as an address and a length.
#[inline(always)]
fn part_of(part: &str, len: *mut usize) -> *const u8 {
    // SAFETY: generated code passes the address of a word of its own.
    unsafe { len.write(part.len()) };
    part.as_ptr()
}

/// The order of two texts: -1, 0, or 1 as the left is less than, equal to,
/// or greater than the right. UTF-8 byte order is `str` order.
///
/// # Safety
///
/// Each address and length name live UTF-8 bytes.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_compare_view(
    left: *const u8,
    left_len: usize,
    right: *const u8,
    right_len: usize,
) -> i64 {
    // SAFETY: the caller's contract.
    let (left, right) = unsafe { (view(left, left_len), view(right, right_len)) };
    left.as_bytes().cmp(right.as_bytes()) as i64
}

/// 1 when `text` starts with `part`, and 0 otherwise.
///
/// # Safety
///
/// Each address and length name live UTF-8 bytes.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_starts_with_view(
    text: *const u8,
    text_len: usize,
    part: *const u8,
    part_len: usize,
) -> i64 {
    // SAFETY: the caller's contract.
    let (text, part) = unsafe { (view(text, text_len), view(part, part_len)) };
    i64::from(text.as_bytes().starts_with(part.as_bytes()))
}

/// 1 when `text` ends with `part`, and 0 otherwise.
///
/// # Safety
///
/// Each address and length name live UTF-8 bytes.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_ends_with_view(
    text: *const u8,
    text_len: usize,
    part: *const u8,
    part_len: usize,
) -> i64 {
    // SAFETY: the caller's contract.
    let (text, part) = unsafe { (view(text, text_len), view(part, part_len)) };
    i64::from(text.as_bytes().ends_with(part.as_bytes()))
}

/// The byte offset of the first `part` in `text`, or -1 when `text` has
/// none. `contains` is an offset that is not negative.
///
/// # Safety
///
/// Each address and length name live UTF-8 bytes.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_find_view(
    text: *const u8,
    text_len: usize,
    part: *const u8,
    part_len: usize,
) -> i64 {
    // SAFETY: the caller's contract.
    let (text, part) = unsafe { (view(text, text_len), view(part, part_len)) };
    let found = match part.as_bytes() {
        [byte] => memchr::memchr(*byte, text.as_bytes()),
        part => memchr::memmem::find(text.as_bytes(), part),
    };
    found.map_or(-1, |offset| offset as i64)
}

/// `text` without the whitespace at its start (`kind` 1), its end (`kind`
/// 2), or both: the address of the part, with its length written to `len`.
///
/// # Safety
///
/// The address and length name live UTF-8 bytes, and `len` addresses a
/// word the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_trim_view(
    text: *const u8,
    text_len: usize,
    kind: i64,
    len: *mut usize,
) -> *const u8 {
    // SAFETY: the caller's contract.
    let text = unsafe { view(text, text_len) };
    let trimmed = match kind {
        1 => text.trim_start(),
        2 => text.trim_end(),
        _ => text.trim(),
    };
    part_of(trimmed, len)
}

/// `parse_int64` on the text: 1 with the number written to `out`, or 0
/// where the parse gives `Err`, with `out` left as it was. No `Result` and
/// no error text is built.
///
/// # Safety
///
/// The address and length name live UTF-8 bytes, and `out` addresses a
/// word the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_parse_int64_view(
    text: *const u8,
    text_len: usize,
    out: *mut i64,
) -> i64 {
    // SAFETY: the caller's contract.
    let text = unsafe { view(text, text_len) };
    match text.parse::<i64>() {
        Ok(value) => {
            // SAFETY: the caller's contract.
            unsafe { out.write(value) };
            1
        }
        Err(_) => 0,
    }
}

/// `parse_int32` on the text, as [`aura_direct_str_parse_int64_view`]
/// gives an `int64`. The number is written as a whole word.
///
/// # Safety
///
/// As [`aura_direct_str_parse_int64_view`].
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_parse_int32_view(
    text: *const u8,
    text_len: usize,
    out: *mut i64,
) -> i64 {
    // SAFETY: the caller's contract.
    let text = unsafe { view(text, text_len) };
    match text.parse::<i32>() {
        Ok(value) => {
            // SAFETY: the caller's contract.
            unsafe { out.write(i64::from(value)) };
            1
        }
        Err(_) => 0,
    }
}

/// `parse_float64` on the text: 1 with the number written to `out`, or 0
/// where the parse gives `Err`, which includes a number that is not
/// finite.
///
/// # Safety
///
/// The address and length name live UTF-8 bytes, and `out` addresses a
/// `float64` the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_parse_float64_view(
    text: *const u8,
    text_len: usize,
    out: *mut f64,
) -> i64 {
    // SAFETY: the caller's contract.
    let text = unsafe { view(text, text_len) };
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => {
            // SAFETY: the caller's contract.
            unsafe { out.write(value) };
            1
        }
        _ => 0,
    }
}

/// The cursor value of a split or a walk over lines that has given its
/// last part.
pub const STR_VIEW_ITERATION_DONE: usize = usize::MAX;

/// The next part of `text.split(separator)`, as `str::split` gives the
/// parts: its address, with its length written to `len`, or null after the
/// last part. `cursor` holds the walk's place between calls. It starts at
/// zero, and the call moves it on. The separator is not empty.
///
/// A loop over the parts makes no list and no value for a part.
///
/// # Safety
///
/// Each address and length name live UTF-8 bytes, the same on every call
/// of one walk, and `cursor` and `len` address words the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_split_next_view(
    text: *const u8,
    text_len: usize,
    separator: *const u8,
    separator_len: usize,
    cursor: *mut usize,
    len: *mut usize,
) -> *const u8 {
    // SAFETY: the caller's contract.
    let (text, separator, start) = unsafe {
        (
            view(text, text_len),
            view(separator, separator_len),
            *cursor,
        )
    };
    if start > text.len() || separator.is_empty() {
        // SAFETY: the caller's contract.
        unsafe { cursor.write(STR_VIEW_ITERATION_DONE) };
        return std::ptr::null();
    }
    let rest = &text.as_bytes()[start..];
    let found = match separator.as_bytes() {
        [byte] => memchr::memchr(*byte, rest),
        separator => memchr::memmem::find(rest, separator),
    };
    let (part_len, next) = match found {
        Some(offset) => (offset, start + offset + separator.len()),
        None => (rest.len(), STR_VIEW_ITERATION_DONE),
    };
    // SAFETY: the caller's contract. The part ends where a separator
    // starts or the text ends, so it is whole scalars.
    unsafe {
        cursor.write(next);
        len.write(part_len);
        text.as_ptr().add(start)
    }
}

/// The next line of `text.lines()`, as `str::lines` gives them: a line
/// ends at `\n` or `\r\n`, neither of which is part of it, and text that
/// ends with a line ending has no empty last line. The address of the
/// line, with its length written to `len`, or null after the last line.
/// `cursor` is as in [`aura_direct_str_split_next_view`].
///
/// # Safety
///
/// As [`aura_direct_str_split_next_view`].
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_lines_next_view(
    text: *const u8,
    text_len: usize,
    cursor: *mut usize,
    len: *mut usize,
) -> *const u8 {
    // SAFETY: the caller's contract.
    let (text, start) = unsafe { (view(text, text_len), *cursor) };
    if start >= text.len() {
        // SAFETY: the caller's contract.
        unsafe { cursor.write(STR_VIEW_ITERATION_DONE) };
        return std::ptr::null();
    }
    let rest = &text.as_bytes()[start..];
    let (line_len, next) = match memchr::memchr(b'\n', rest) {
        // A carriage return belongs to the line ending only before a
        // line feed.
        Some(offset) if offset > 0 && rest[offset - 1] == b'\r' => (offset - 1, start + offset + 1),
        Some(offset) => (offset, start + offset + 1),
        None => (rest.len(), STR_VIEW_ITERATION_DONE),
    };
    // SAFETY: the caller's contract. The line ends before an ASCII line
    // ending or at the end of the text, so it is whole scalars.
    unsafe {
        cursor.write(next);
        len.write(line_len);
        text.as_ptr().add(start)
    }
}

/// A walk over the parts `text.split(separator)` gives.
pub const STR_PARTS_SPLIT: i64 = 0;
/// A walk over the lines `text.lines()` gives.
pub const STR_PARTS_LINES: i64 = 1;
/// A walk over the words `text.split_whitespace()` gives.
pub const STR_PARTS_WHITESPACE: i64 = 2;

/// The part of `text` a walk of `kind` gives at the place `start`, with
/// the place of the part after it, or `None` after the last part. A walk
/// starts at zero. The parts are, in order, the elements of the list the
/// `str` method of that kind makes.
#[inline]
fn next_part<'a>(
    text: &'a str,
    separator: &str,
    kind: i64,
    start: usize,
) -> Option<(&'a str, usize)> {
    let bytes = text.as_bytes();
    // A part of a split or of the lines ends where a separator or a line
    // ending starts or the text ends, and starts where one ends or the
    // text starts, so it is whole scalars.
    let part = |from: usize, to: usize| {
        // SAFETY: the bounds are scalar boundaries of `text`, as above.
        unsafe { std::str::from_utf8_unchecked(&bytes[from..to]) }
    };
    match kind {
        STR_PARTS_SPLIT => {
            let rest = bytes.get(start..)?;
            let found = match separator.as_bytes() {
                [] => return None,
                [byte] => memchr::memchr(*byte, rest),
                separator => memchr::memmem::find(rest, separator),
            };
            Some(match found {
                Some(offset) => (
                    part(start, start + offset),
                    start + offset + separator.len(),
                ),
                None => (part(start, bytes.len()), STR_VIEW_ITERATION_DONE),
            })
        }
        STR_PARTS_LINES => {
            let rest = bytes.get(start..).filter(|rest| !rest.is_empty())?;
            Some(match memchr::memchr(b'\n', rest) {
                // A carriage return belongs to the line ending only
                // before a line feed.
                Some(offset) if offset > 0 && rest[offset - 1] == b'\r' => {
                    (part(start, start + offset - 1), start + offset + 1)
                }
                Some(offset) => (part(start, start + offset), start + offset + 1),
                None => (part(start, bytes.len()), STR_VIEW_ITERATION_DONE),
            })
        }
        _ => {
            // The place is the end of the part before, a scalar boundary.
            let rest = text.get(start..)?.as_bytes();
            // Every white space scalar outside ASCII is more than one
            // byte, so a word of ASCII bytes between ASCII white space or
            // the ends of the text is found a byte at a time. The first
            // byte outside ASCII hands the word to the general walk,
            // which starts at the word's first byte, a scalar boundary.
            let space = |byte: u8| byte == b' ' || (b'\t'..=b'\r').contains(&byte);
            let from = rest.iter().position(|byte| !space(*byte))?;
            let word = &rest[from..];
            let len = word
                .iter()
                .position(|byte| space(*byte) || !byte.is_ascii())
                .unwrap_or(word.len());
            if word.get(len).is_none_or(u8::is_ascii) {
                let from = start + from;
                return Some((part(from, from + len), from + len));
            }
            let part = text.get(start + from..)?.split_whitespace().next()?;
            let offset = part.as_ptr() as usize - text.as_ptr() as usize;
            Some((part, offset + part.len()))
        }
    }
}

/// How many parts a walk of `kind` over `text` gives: the length of the
/// list `text.split(separator)`, `text.lines()`, or
/// `text.split_whitespace()` makes, for [`STR_PARTS_SPLIT`],
/// [`STR_PARTS_LINES`], and [`STR_PARTS_WHITESPACE`]. Only a split reads
/// the separator, which is not empty.
///
/// # Safety
///
/// Each address and length name live UTF-8 bytes.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_parts_count_view(
    text: *const u8,
    text_len: usize,
    separator: *const u8,
    separator_len: usize,
    kind: i64,
) -> i64 {
    // SAFETY: the caller's contract.
    let (text, separator) = unsafe { (view(text, text_len), view(separator, separator_len)) };
    // One byte to look for is counted without a stop at each part: a
    // split gives one part more than it finds separators, and the lines
    // are one more than the line feeds unless the text ends at one.
    match (kind, separator.as_bytes()) {
        (STR_PARTS_SPLIT, [byte]) => {
            return memchr::memchr_iter(*byte, text.as_bytes()).count() as i64 + 1;
        }
        (STR_PARTS_LINES, _) => {
            let ended = text.as_bytes().last().is_none_or(|last| *last == b'\n');
            return memchr::memchr_iter(b'\n', text.as_bytes()).count() as i64 + i64::from(!ended);
        }
        _ => {}
    }
    let (mut count, mut place) = (0, 0);
    while let Some((_, next)) = next_part(text, separator, kind, place) {
        count += 1;
        place = next;
    }
    count
}

/// The next part of a walk of `kind` over `text` (see
/// [`aura_direct_str_parts_count_view`]): its address, with its length
/// written to `len`. `cursor` holds the walk's place between calls. It
/// starts at zero, and the call moves it on. After the last part the
/// result is no address, with a length of zero: a part, even an empty
/// one, lies in the text and has one.
///
/// A loop over the parts makes no list and no value for a part.
///
/// # Safety
///
/// Each address and length name live UTF-8 bytes, the same on every call
/// of one walk, and `cursor` and `len` address words the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_parts_next_view(
    text: *const u8,
    text_len: usize,
    separator: *const u8,
    separator_len: usize,
    kind: i64,
    cursor: *mut usize,
    len: *mut usize,
) -> *const u8 {
    // SAFETY: the caller's contract.
    let (text, separator, start) = unsafe {
        (
            view(text, text_len),
            view(separator, separator_len),
            *cursor,
        )
    };
    let Some((part, next)) = next_part(text, separator, kind, start) else {
        // SAFETY: the caller's contract.
        unsafe {
            cursor.write(STR_VIEW_ITERATION_DONE);
            len.write(0);
        }
        return std::ptr::null();
    };
    // SAFETY: the caller's contract.
    unsafe { cursor.write(next) };
    part_of(part, len)
}

/// A text being built in three words the caller owns: the address, the
/// length and the capacity of its bytes. Three zero words are the empty
/// buffer, which owns nothing. The caller ends every buffer exactly once,
/// with [`aura_direct_str_buffer_finish`] or
/// [`aura_direct_str_buffer_free`].
#[repr(C)]
pub struct DirectStrBuffer {
    ptr: *mut u8,
    len: usize,
    capacity: usize,
}

impl DirectStrBuffer {
    /// Takes the text out, leaving the empty buffer.
    ///
    /// # Safety
    ///
    /// The words are zero or what [`Self::put`] wrote.
    unsafe fn take(&mut self) -> String {
        let text = if self.capacity == 0 {
            String::new()
        } else {
            // SAFETY: `put` wrote the parts of a `String` it forgot.
            unsafe { String::from_raw_parts(self.ptr, self.len, self.capacity) }
        };
        *self = Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
        };
        text
    }

    fn put(&mut self, text: String) {
        let mut text = std::mem::ManuallyDrop::new(text);
        *self = Self {
            ptr: text.as_mut_ptr(),
            len: text.len(),
            capacity: text.capacity(),
        };
    }

    /// Appends `part`, or stops the program when the text would pass the
    /// string limit. The buffer keeps its text either way, so the cleanup
    /// that follows a failure frees it.
    ///
    /// # Safety
    ///
    /// As [`Self::take`].
    unsafe fn push(&mut self, part: &str) {
        // SAFETY: the caller's contract.
        let mut text = unsafe { self.take() };
        let appended = crate::runtime_value::append_string_checked(&mut text, part);
        self.put(text);
        if let Err(error) = appended {
            super::task_runtime_boundary(|| super::runtime_diagnostic_error(error))
        }
    }
}

/// Appends the text at `part` to `buffer`: one part of an f-string or one
/// side of `+`, with no value made for the part or for the text so far.
///
/// # Safety
///
/// `buffer` addresses a buffer the caller owns, and the address and length
/// name live UTF-8 bytes outside it.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_buffer_push_view(
    buffer: *mut DirectStrBuffer,
    part: *const u8,
    part_len: usize,
) {
    // SAFETY: the caller's contract.
    unsafe { (*buffer).push(view(part, part_len)) }
}

/// Appends `value` in decimal to `buffer`, as `{value}` formats an
/// `int64`.
///
/// # Safety
///
/// `buffer` addresses a buffer the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_buffer_push_i64(
    buffer: *mut DirectStrBuffer,
    value: i64,
) {
    let mut digits = [0u8; super::I64_DECIMAL_BYTES];
    // SAFETY: the caller's contract.
    unsafe { (*buffer).push(super::i64_decimal(value, &mut digits)) }
}

/// Makes room in `buffer` for `additional` more bytes, so the appends that
/// follow grow it at most once. A hint: it changes no text.
///
/// # Safety
///
/// `buffer` addresses a buffer the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_buffer_reserve(
    buffer: *mut DirectStrBuffer,
    additional: usize,
) {
    // SAFETY: the caller's contract.
    unsafe {
        let mut text = (*buffer).take();
        text.reserve(additional.min(super::FORMAT_RESERVE_LIMIT));
        (*buffer).put(text);
    }
}

/// The `str` box of the text in `buffer`, which is left empty.
///
/// # Safety
///
/// `buffer` addresses a buffer the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_buffer_finish(
    buffer: *mut DirectStrBuffer,
) -> *mut super::OpaqueValue {
    // SAFETY: the caller's contract.
    let text = unsafe { (*buffer).take() };
    super::task_runtime_boundary(|| {
        super::boxed_value(crate::runtime_value::Value::String(text.into()))
    })
}

/// Frees the text in `buffer`, which is left empty: the end of a buffer
/// whose text was only read, or the cleanup of one a failure interrupted.
///
/// # Safety
///
/// `buffer` addresses a buffer the caller owns.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_str_buffer_free(buffer: *mut DirectStrBuffer) {
    // SAFETY: the caller's contract.
    drop(unsafe { (*buffer).take() });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of<'a>(ptr: *const u8, len: usize) -> &'a str {
        unsafe { view(ptr, len) }
    }

    fn split_all(text: &str, separator: &str) -> Vec<String> {
        let (mut cursor, mut len) = (0usize, 0usize);
        let mut parts = Vec::new();
        loop {
            let part = unsafe {
                aura_direct_str_split_next_view(
                    text.as_ptr(),
                    text.len(),
                    separator.as_ptr(),
                    separator.len(),
                    &mut cursor,
                    &mut len,
                )
            };
            if part.is_null() {
                assert_eq!(cursor, STR_VIEW_ITERATION_DONE);
                return parts;
            }
            parts.push(text_of(part, len).to_string());
        }
    }

    fn parts_all(text: &str, separator: &str, kind: i64) -> Vec<String> {
        let count = unsafe {
            aura_direct_str_parts_count_view(
                text.as_ptr(),
                text.len(),
                separator.as_ptr(),
                separator.len(),
                kind,
            )
        };
        let (mut cursor, mut len) = (0usize, 0usize);
        let mut next = || {
            let part = unsafe {
                aura_direct_str_parts_next_view(
                    text.as_ptr(),
                    text.len(),
                    separator.as_ptr(),
                    separator.len(),
                    kind,
                    &mut cursor,
                    &mut len,
                )
            };
            (!part.is_null()).then(|| text_of(part, len).to_string())
        };
        // Every part has an address, an empty one too.
        let parts = (0..count)
            .map(|_| next().expect("a part for each one counted"))
            .collect::<Vec<_>>();
        // A walk past its last part gives no address, however often.
        assert_eq!(next(), None);
        assert_eq!(next(), None);
        assert_eq!((cursor, len), (STR_VIEW_ITERATION_DONE, 0));
        parts
    }

    #[test]
    fn a_counted_walk_gives_the_parts_the_list_would_hold() {
        let texts = [
            "",
            "a",
            "a,b",
            ",a,,b,",
            ",",
            "one two  three\tfour\n",
            "  lead and trail  ",
            " \u{2003}wide\u{a0}spaces\u{3000}here ",
            "\u{a0}lead",
            "tail\u{2003}",
            "plain é mixed\u{a0}in x\u{85}y \u{1c}z\u{b}w\u{c}v",
            "é",
            " \u{3000} ",
            "a\u{a0}",
            "line\nline two\r\nthree\n\nfive\r",
            "\n",
            "\r\n",
            "\n\nx",
            "žluťoučký kůň\núpěl",
            "a--b----c--",
        ];
        for text in texts {
            for separator in [",", " ", "\n", "--", "ů", "line"] {
                assert_eq!(
                    parts_all(text, separator, STR_PARTS_SPLIT),
                    text.split(separator).collect::<Vec<_>>(),
                    "{text:?} split by {separator:?}"
                );
            }
            let list = |value: crate::runtime_value::Value| match value {
                crate::runtime_value::Value::Vec(vector) => vector
                    .elements
                    .iter()
                    .map(|element| match element.as_ref() {
                        crate::runtime_value::Value::String(part) => part.as_str().to_string(),
                        other => panic!("expected a str, found {other:?}"),
                    })
                    .collect::<Vec<_>>(),
                other => panic!("expected a list, found {other:?}"),
            };
            assert_eq!(
                parts_all(text, "", STR_PARTS_LINES),
                list(crate::text_methods::lines(text)),
                "{text:?} lines"
            );
            assert_eq!(
                parts_all(text, "", STR_PARTS_WHITESPACE),
                list(crate::text_methods::split_whitespace(text)),
                "{text:?} words"
            );
            // A split by an empty separator is never compiled this way,
            // and gives no parts.
            assert!(parts_all(text, "", STR_PARTS_SPLIT).is_empty());
        }
    }

    fn lines_all(text: &str) -> Vec<String> {
        let (mut cursor, mut len) = (0usize, 0usize);
        let mut lines = Vec::new();
        loop {
            let line = unsafe {
                aura_direct_str_lines_next_view(text.as_ptr(), text.len(), &mut cursor, &mut len)
            };
            if line.is_null() {
                return lines;
            }
            lines.push(text_of(line, len).to_string());
        }
    }

    #[test]
    fn views_compare_and_search_as_str_does() {
        let pairs = [
            ("", ""),
            ("a", ""),
            ("", "a"),
            ("abc", "abd"),
            ("abc", "ab"),
            ("é", "e"),
            ("héllo wörld", "wör"),
            ("aaa", "aa"),
            ("x", "x"),
            ("line one", " "),
        ];
        for (left, right) in pairs {
            let call = |entry: unsafe extern "C-unwind" fn(
                *const u8,
                usize,
                *const u8,
                usize,
            ) -> i64| unsafe {
                entry(left.as_ptr(), left.len(), right.as_ptr(), right.len())
            };
            assert_eq!(
                call(aura_direct_str_compare_view),
                left.cmp(right) as i64,
                "{left:?} {right:?}"
            );
            assert_eq!(
                call(aura_direct_str_starts_with_view),
                i64::from(left.starts_with(right))
            );
            assert_eq!(
                call(aura_direct_str_ends_with_view),
                i64::from(left.ends_with(right))
            );
            assert_eq!(
                call(aura_direct_str_find_view),
                left.find(right).map_or(-1, |offset| offset as i64),
                "{left:?} {right:?}"
            );
        }
        // An empty text is read from no address at all.
        let nothing = unsafe { aura_direct_str_compare_view(std::ptr::null(), 0, "a".as_ptr(), 1) };
        assert_eq!(nothing, -1);
    }

    #[test]
    fn a_trimmed_view_lies_inside_its_text() {
        for text in ["", "  ", " a b \n", "é ", "\tx", "plain"] {
            for (kind, expected) in [
                (0, text.trim()),
                (1, text.trim_start()),
                (2, text.trim_end()),
            ] {
                let mut len = usize::MAX;
                let part =
                    unsafe { aura_direct_str_trim_view(text.as_ptr(), text.len(), kind, &mut len) };
                assert_eq!(text_of(part, len), expected, "{text:?} {kind}");
                let offset = part as usize - text.as_ptr() as usize;
                assert!(offset + len <= text.len(), "{text:?} {kind}");
            }
        }
    }

    #[test]
    fn number_views_parse_as_the_boxed_parses_do() {
        for text in [
            "0",
            "-17",
            "+5",
            "9223372036854775807",
            "9223372036854775808",
            "",
            " 1",
            "1 ",
            "1.5",
            "x",
        ] {
            let mut out = 77i64;
            let ok =
                unsafe { aura_direct_str_parse_int64_view(text.as_ptr(), text.len(), &mut out) };
            match text.parse::<i64>() {
                Ok(value) => assert_eq!((ok, out), (1, value), "{text:?}"),
                Err(_) => assert_eq!((ok, out), (0, 77), "{text:?}"),
            }
        }
        for text in ["2147483647", "2147483648", "-2147483648", "12", "z"] {
            let mut out = 77i64;
            let ok =
                unsafe { aura_direct_str_parse_int32_view(text.as_ptr(), text.len(), &mut out) };
            match text.parse::<i32>() {
                Ok(value) => assert_eq!((ok, out), (1, i64::from(value)), "{text:?}"),
                Err(_) => assert_eq!((ok, out), (0, 77), "{text:?}"),
            }
        }
        for text in [
            "1.5", "-0.25", "1e3", "7", "inf", "NaN", "1e999", "", "1.5x",
        ] {
            let mut out = 77.0f64;
            let ok =
                unsafe { aura_direct_str_parse_float64_view(text.as_ptr(), text.len(), &mut out) };
            match text.parse::<f64>().ok().filter(|value| value.is_finite()) {
                Some(value) => assert_eq!((ok, out), (1, value), "{text:?}"),
                None => assert_eq!((ok, out), (0, 77.0), "{text:?}"),
            }
        }
    }

    #[test]
    fn a_split_walk_gives_the_parts_split_gives() {
        let cases = [
            ("a,b,c", ","),
            ("", ","),
            (",", ","),
            ("a,,b,", ","),
            ("2026-01-02", "-"),
            ("a--b--", "--"),
            ("none", ";"),
            ("é,ü", ","),
            ("x→y→z", "→"),
            ("aaa", "aa"),
        ];
        for (text, separator) in cases {
            let expected = text
                .split(separator)
                .map(str::to_string)
                .collect::<Vec<_>>();
            assert_eq!(
                split_all(text, separator),
                expected,
                "{text:?} {separator:?}"
            );
        }
        // An empty separator gives no parts, and a finished walk stays
        // finished.
        assert_eq!(split_all("abc", ""), Vec::<String>::new());
        let (mut cursor, mut len) = (STR_VIEW_ITERATION_DONE, 0usize);
        let part = unsafe {
            aura_direct_str_split_next_view("a".as_ptr(), 1, ",".as_ptr(), 1, &mut cursor, &mut len)
        };
        assert!(part.is_null());
    }

    #[test]
    fn a_line_walk_gives_the_lines_lines_gives() {
        for text in [
            "",
            "one",
            "one\n",
            "one\ntwo",
            "one\r\ntwo\r\n",
            "\n\nx\n",
            "a\rb\n",
            "é\nü\n",
            "\r\n",
            "end\r",
        ] {
            let expected = text.lines().map(str::to_string).collect::<Vec<_>>();
            assert_eq!(lines_all(text), expected, "{text:?}");
        }
    }

    #[test]
    fn a_buffer_collects_parts_and_becomes_a_str() {
        let mut buffer = DirectStrBuffer {
            ptr: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
        };
        let push = |buffer: &mut DirectStrBuffer, part: &str| unsafe {
            aura_direct_str_buffer_push_view(buffer, part.as_ptr(), part.len());
        };
        unsafe { aura_direct_str_buffer_reserve(&mut buffer, 8) };
        assert!(buffer.capacity >= 8 && buffer.len == 0);
        push(&mut buffer, "héllo ");
        push(&mut buffer, "");
        unsafe {
            aura_direct_str_buffer_push_i64(&mut buffer, -42);
            aura_direct_str_buffer_push_i64(&mut buffer, i64::MIN);
        }
        push(
            &mut buffer,
            " a part long enough to move the text to a larger block",
        );
        let expected = format!(
            "héllo -42{} a part long enough to move the text to a larger block",
            i64::MIN
        );
        assert_eq!(text_of(buffer.ptr, buffer.len), expected);
        let boxed = unsafe { aura_direct_str_buffer_finish(&mut buffer) };
        assert_eq!((buffer.len, buffer.capacity), (0, 0));
        let value = unsafe { super::super::take_value(boxed) };
        assert_eq!(
            value,
            crate::runtime_value::Value::String(expected.as_str().into())
        );
        unsafe { super::super::aura_direct_release_value(boxed) };

        // An empty buffer finishes as the empty text, and a freed buffer
        // can be used again.
        let empty = unsafe { aura_direct_str_buffer_finish(&mut buffer) };
        let value = unsafe { super::super::take_value(empty) };
        assert_eq!(value, crate::runtime_value::Value::String("".into()));
        unsafe { super::super::aura_direct_release_value(empty) };
        push(&mut buffer, "dropped");
        unsafe { aura_direct_str_buffer_free(&mut buffer) };
        assert_eq!((buffer.len, buffer.capacity), (0, 0));
        unsafe { aura_direct_str_buffer_free(&mut buffer) };
    }
}
