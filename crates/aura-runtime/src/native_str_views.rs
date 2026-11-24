//! Text entry points that read their input where it lies.
//!
//! Each takes a `str` as the address and length of its UTF-8 bytes, and
//! none takes or makes a box: generated code that holds a literal, an
//! element of a `list[str]` view, or a `str` kept as words calls them
//! without first building a value. A result that is part of the input comes
//! back as an address and a length inside it, so the caller keeps the input
//! alive for as long as it reads the result.
//!
//! None of them can fail, so none crosses the runtime boundary.

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
}
