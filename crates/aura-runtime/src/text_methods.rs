//! Shared runtime semantics for the text and byte convenience methods.
//!
//! The MIR interpreter and the direct native runtime both call these helpers,
//! so the two backends produce the same values and the same traps. Each
//! helper reads shared inputs and returns a fresh owned value.

use crate::diag::{Diagnostic, Result};
use crate::integer::IntegerValue;
use crate::runtime_value::{
    host_bytes_from_runtime, optional_kept, optional_named, SharedType, TupleValue, Value,
    VecValue, MAX_STRING_BYTES,
};
use aura_core::types::Type;

/// The `str` text-convenience methods, in the order the direct backend
/// numbers them for its runtime dispatcher.
pub const TEXT_METHOD_NAMES: [&str; 11] = [
    "trim_start",
    "trim_end",
    "split_once",
    "rsplit_once",
    "lines",
    "split_whitespace",
    "find",
    "count",
    "repeat",
    "chars",
    "char_at",
];

/// Evaluates one text-convenience method by name. `argument` is the text
/// argument of `split_once`, `rsplit_once`, `find`, and `count`; `count` is
/// the scalar argument of `repeat`.
pub fn evaluate_text_method(
    method: &str,
    text: &str,
    argument: Option<&str>,
    count: i64,
) -> Result<Value> {
    let argument = || {
        argument.ok_or_else(|| {
            Diagnostic::coded("AU4001", format!("`{method}` requires a `str` argument"))
        })
    };
    match method {
        "trim_start" => Ok(trim_start(text)),
        "trim_end" => Ok(trim_end(text)),
        "split_once" => split_once(text, argument()?, false),
        "rsplit_once" => split_once(text, argument()?, true),
        "lines" => Ok(lines(text)),
        "split_whitespace" => Ok(split_whitespace(text)),
        "find" => Ok(find(text, argument()?)),
        "count" => count_occurrences(text, argument()?),
        "repeat" => repeat(text, count),
        "chars" => chars(text),
        "char_at" => char_at(text, text.is_ascii(), count),
        _ => Err(Diagnostic::coded(
            "AU4001",
            format!("unknown text method `str.{method}`"),
        )),
    }
}

fn str_type() -> Type {
    Type::named("str")
}

fn int64_value(value: usize) -> Value {
    Value::Int(IntegerValue::from_i64(
        i64::try_from(value).expect("text and byte offsets fit in int64"),
    ))
}

fn string_list<T: Into<crate::text::Text>>(parts: impl IntoIterator<Item = T>) -> Value {
    Value::Vec(VecValue {
        element_type: SharedType::from(str_type()),
        elements: parts.into_iter().map(Value::string).collect(),
        str_view: Default::default(),
    })
}

fn optional_index(index: Option<usize>) -> Value {
    optional_named("int64", index.map(int64_value))
}

/// `str.trim_start()`: removes leading Unicode whitespace.
pub fn trim_start(text: &str) -> Value {
    Value::String(crate::text::Text::from(text.trim_start()))
}

/// `str.trim_end()`: removes trailing Unicode whitespace.
pub fn trim_end(text: &str) -> Value {
    Value::String(crate::text::Text::from(text.trim_end()))
}

/// `str.split_once(separator)` and `str.rsplit_once(separator)`.
///
/// Returns `(before, after) | None`. An empty separator traps with `AU4003`.
pub fn split_once(text: &str, separator: &str, from_end: bool) -> Result<Value> {
    let method = if from_end {
        "rsplit_once"
    } else {
        "split_once"
    };
    if separator.is_empty() {
        return Err(Diagnostic::coded(
            "AU4003",
            format!("`{method}` separator cannot be empty"),
        ));
    }
    let split = if from_end {
        text.rsplit_once(separator)
    } else {
        text.split_once(separator)
    };
    let pair_type = || Type::Tuple(vec![str_type(), str_type()]);
    Ok(optional_kept(
        "(str, str)",
        &pair_type,
        split.map(|(before, after)| {
            Value::Tuple(TupleValue {
                element_types: vec![str_type(), str_type()],
                elements: vec![
                    Value::String(crate::text::Text::from(before)),
                    Value::String(crate::text::Text::from(after)),
                ],
            })
        }),
    ))
}

/// `str.lines()`: splits at `\n` and removes one `\r` before each `\n`.
/// A final line terminator adds no empty line, and an empty string has no
/// lines.
pub fn lines(text: &str) -> Value {
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.split_once('\n') {
            Some((line, after)) => {
                parts.push(line.strip_suffix('\r').unwrap_or(line));
                rest = after;
            }
            None => {
                parts.push(rest);
                break;
            }
        }
    }
    string_list(parts)
}

/// `str.chars()`: each Unicode scalar value as a one-character string.
/// The list is reserved fallibly, so a text too long for the memory left
/// traps with `AU4005`.
pub fn chars(text: &str) -> Result<Value> {
    let count = if text.is_ascii() {
        text.len()
    } else {
        text.chars().count()
    };
    let mut parts: Vec<Value> = Vec::new();
    parts
        .try_reserve_exact(count)
        .map_err(|_| string_allocation_failed())?;
    parts.extend(text.chars().map(|ch| Value::string(String::from(ch))));
    Ok(Value::Vec(VecValue {
        element_type: SharedType::from(str_type()),
        elements: parts.into(),
        str_view: Default::default(),
    }))
}

fn string_allocation_failed() -> Diagnostic {
    Diagnostic::coded("AU4005", "string allocation failed")
}

fn string_too_long() -> Diagnostic {
    Diagnostic::coded(
        "AU4005",
        format!("string output exceeds the maintained {MAX_STRING_BYTES}-byte allocation limit"),
    )
}

/// An empty string with room for `len` bytes, when `len` (which is `None`
/// when it overflowed) fits the string limit and the memory can be had.
fn reserved_string(len: Option<usize>) -> Result<String> {
    let len = len
        .filter(|len| *len <= MAX_STRING_BYTES)
        .ok_or_else(string_too_long)?;
    let mut output = String::new();
    output
        .try_reserve_exact(len)
        .map_err(|_| string_allocation_failed())?;
    Ok(output)
}

/// `separator.join(parts)`, refused with `AU4005` before it is built when
/// the result is over the string limit.
pub fn join_checked<S: AsRef<str>>(parts: &[S], separator: &str) -> Result<String> {
    let separators = parts.len().saturating_sub(1);
    let len = parts
        .iter()
        .try_fold(0usize, |len, part| len.checked_add(part.as_ref().len()))
        .and_then(|len| separator.len().checked_mul(separators)?.checked_add(len));
    let mut output = reserved_string(len)?;
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            output.push_str(separator);
        }
        output.push_str(part.as_ref());
    }
    Ok(output)
}

/// An upper bound on the length of `text.replace(from, to)` that fits the
/// string limit, or `AU4005` when the result is over it. The matches are
/// counted only when the most matches the text could hold might reach it.
pub fn replaced_len_checked(text: &str, from: &str, to: &str) -> Result<usize> {
    // An empty pattern matches at every character boundary; others do not
    // overlap, so they cover at most the whole text.
    let most_matches = text.len() / from.len().max(1) + 1;
    let most = to
        .len()
        .checked_mul(most_matches)
        .and_then(|added| added.checked_add(text.len()));
    if most.is_some_and(|most| most <= MAX_STRING_BYTES) {
        return Ok(most.unwrap_or_default());
    }
    let matches = if from.is_empty() {
        text.chars().count() + 1
    } else {
        memchr::memmem::find_iter(text.as_bytes(), from.as_bytes()).count()
    };
    to.len()
        .checked_mul(matches)
        .and_then(|added| (text.len() - matches * from.len()).checked_add(added))
        .filter(|len| *len <= MAX_STRING_BYTES)
        .ok_or_else(string_too_long)
}

/// `text.replace(from, to)`, refused with `AU4005` before it is built when
/// the result is over the string limit.
pub fn replace_checked(text: &str, from: &str, to: &str) -> Result<String> {
    replaced_len_checked(text, from, to)?;
    let mut output = reserved_string(Some(text.len()))?;
    if from.is_empty() {
        output.push_str(to);
        for ch in text.chars() {
            output.push(ch);
            output.push_str(to);
        }
        return Ok(output);
    }
    let mut rest = 0;
    for position in memchr::memmem::find_iter(text.as_bytes(), from.as_bytes()) {
        output.push_str(&text[rest..position]);
        output.push_str(to);
        rest = position + from.len();
    }
    output.push_str(&text[rest..]);
    Ok(output)
}

/// `text.to_lower()` or, with `upper`, `text.to_upper()`, refused with
/// `AU4005` before it is built when the result is over the string limit.
/// A character's case mapping can be longer than the character.
pub fn change_case_checked(text: &str, upper: bool) -> Result<String> {
    if text.is_ascii() {
        let mut output = reserved_string(Some(text.len()))?;
        output.push_str(text);
        if upper {
            output.make_ascii_uppercase();
        } else {
            output.make_ascii_lowercase();
        }
        return Ok(output);
    }
    let len = text.chars().try_fold(0usize, |len, ch| {
        let mapped: usize = if upper {
            ch.to_uppercase().map(char::len_utf8).sum()
        } else {
            ch.to_lowercase().map(char::len_utf8).sum()
        };
        len.checked_add(mapped)
    });
    // `str::to_lowercase` maps a final sigma by its context, which keeps
    // its length, so the count above is exact.
    if len.is_none_or(|len| len > MAX_STRING_BYTES) {
        return Err(string_too_long());
    }
    Ok(if upper {
        text.to_uppercase()
    } else {
        text.to_lowercase()
    })
}

/// `ord(text)`: the Unicode scalar value of a one-character string. Any
/// other length traps with `AU4003`.
pub fn ord(text: &str) -> Result<i64> {
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => Ok(i64::from(u32::from(character))),
        _ => Err(Diagnostic::coded(
            "AU4003",
            format!(
                "`ord(...)` expects a one-character `str`, found {} characters",
                text.chars().count()
            ),
        )),
    }
}

/// `chr(code)`: the one-character string for a Unicode scalar value. A
/// surrogate or out-of-range code traps with `AU4003`.
pub fn chr(code: i64) -> Result<Value> {
    u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .map(|character| Value::String(String::from(character).into()))
        .ok_or_else(|| {
            Diagnostic::coded(
                "AU4003",
                format!("`chr(...)` expects a Unicode scalar value, found {code}"),
            )
        })
}

/// `str.split_whitespace()`: splits on runs of Unicode whitespace and drops
/// empty parts.
pub fn split_whitespace(text: &str) -> Value {
    string_list(text.split_whitespace())
}

/// `str.find(text)`: the index of the first occurrence, counted in Unicode
/// scalar values. An empty needle is found at index `0`.
pub fn find(text: &str, needle: &str) -> Value {
    optional_index(
        text.find(needle)
            .map(|byte_index| text[..byte_index].chars().count()),
    )
}

/// `str.count(text)`: the number of non-overlapping occurrences. An empty
/// needle traps with `AU4003`.
pub fn count_occurrences(text: &str, needle: &str) -> Result<Value> {
    if needle.is_empty() {
        return Err(Diagnostic::coded("AU4003", "`count` text cannot be empty"));
    }
    // Non-overlapping matches, as `str::matches` counts them.
    let count = match needle.as_bytes() {
        [byte] => memchr::memchr_iter(*byte, text.as_bytes()).count(),
        needle => memchr::memmem::find_iter(text.as_bytes(), needle).count(),
    };
    Ok(int64_value(count))
}

/// `str.repeat(count)`: `count` concatenated copies. A negative count traps
/// with `AU4003`. A result above the string limit traps with `AU4005` before
/// any allocation.
pub fn repeat(text: &str, count: i64) -> Result<Value> {
    let Ok(count) = usize::try_from(count) else {
        return Err(Diagnostic::coded(
            "AU4003",
            format!("`repeat` count cannot be negative, found {count}"),
        ));
    };
    let required = text
        .len()
        .checked_mul(count)
        .filter(|required| *required <= MAX_STRING_BYTES);
    let Some(required) = required else {
        return Err(Diagnostic::coded(
            "AU4005",
            format!("`repeat` result exceeds the maintained {MAX_STRING_BYTES}-byte string limit"),
        ));
    };
    let mut output = String::new();
    output
        .try_reserve_exact(required)
        .map_err(|_| Diagnostic::coded("AU4005", "string allocation failed"))?;
    if !text.is_empty() {
        for _ in 0..count {
            output.push_str(text);
        }
    }
    Ok(Value::String(output.into()))
}

/// Byte search helpers for `bytes.starts_with`, `bytes.ends_with`, and
/// `bytes.find`.
pub fn is_bytes_search_builtin(name: &str) -> bool {
    matches!(
        name,
        "bytes::starts_with" | "bytes::ends_with" | "bytes::find"
    )
}

/// Copies one shared `list[uint8]` argument of a byte search builtin.
pub fn bytes_search_operand(name: &str, value: &Value) -> Result<Vec<u8>> {
    host_bytes_from_runtime(value, &name.replace("::", "."))
}

/// Evaluates one byte search builtin over two copied byte operands.
pub fn evaluate_bytes_search(name: &str, value: &[u8], pattern: &[u8]) -> Value {
    match name {
        "bytes::starts_with" => Value::Bool(value.starts_with(pattern)),
        "bytes::ends_with" => Value::Bool(value.ends_with(pattern)),
        _ => optional_index(memchr::memmem::find(value, pattern)),
    }
}

/// `text[index]`: the scalar at `index` as a one-character string. A
/// negative index counts from the end. ASCII text, which `ascii` reports,
/// is read at the byte; other text is walked from the nearer end.
pub fn char_at(text: &str, ascii: bool, index: i64) -> Result<Value> {
    scalar_at(text, ascii, index).map(Value::string)
}

/// Selects a Unicode scalar without materializing a one-character string.
pub(crate) fn scalar_at(text: &str, ascii: bool, index: i64) -> Result<char> {
    let out_of_range = |length: usize| {
        Diagnostic::coded(
            "AU4003",
            format!("str index {index} is out of range for a string of {length} characters"),
        )
    };
    if ascii {
        let length = text.len();
        let position = if index < 0 {
            index.checked_add(length as i64)
        } else {
            Some(index)
        };
        return match position.and_then(|position| usize::try_from(position).ok()) {
            Some(position) if position < length => Ok(char::from(text.as_bytes()[position])),
            _ => Err(out_of_range(length)),
        };
    }
    let found = if index < 0 {
        index
            .checked_neg()
            .and_then(|back| usize::try_from(back - 1).ok())
            .and_then(|back| text.chars().nth_back(back))
    } else {
        usize::try_from(index)
            .ok()
            .and_then(|position| text.chars().nth(position))
    };
    match found {
        Some(scalar) => Ok(scalar),
        None => Err(out_of_range(text.chars().count())),
    }
}

/// The opcode generated code passes for a `str` text method.
pub fn text_method_opcode(field: &str) -> Option<i64> {
    TEXT_METHOD_NAMES
        .iter()
        .position(|name| *name == field)
        .map(|index| index as i64)
}

#[cfg(test)]
#[path = "text_methods_tests.rs"]
mod tests;
