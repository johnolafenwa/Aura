//! Shared runtime semantics for the text and byte convenience methods.
//!
//! The MIR interpreter and the direct native runtime both call these helpers,
//! so the two backends produce the same values and the same traps. Each
//! helper reads shared inputs and returns a fresh owned value.

use crate::diag::{Diagnostic, Result};
use crate::integer::IntegerValue;
use crate::runtime_value::{
    host_bytes_from_runtime, optional_value, TupleValue, Value, VecValue, MAX_STRING_BYTES,
};
use crate::sema::Type;

/// The `str` text-convenience methods, in the order the direct backend
/// numbers them for its runtime dispatcher.
pub(crate) const TEXT_METHOD_NAMES: [&str; 9] = [
    "trim_start",
    "trim_end",
    "split_once",
    "rsplit_once",
    "lines",
    "split_whitespace",
    "find",
    "count",
    "repeat",
];

/// Evaluates one text-convenience method by name. `argument` is the text
/// argument of `split_once`, `rsplit_once`, `find`, and `count`; `count` is
/// the scalar argument of `repeat`.
pub(crate) fn evaluate_text_method(
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

fn string_list(parts: impl IntoIterator<Item = String>) -> Value {
    Value::Vec(VecValue {
        element_type: str_type(),
        elements: parts.into_iter().map(Value::String).collect(),
    })
}

fn optional_index(index: Option<usize>) -> Value {
    optional_value(Type::named("int64"), index.map(int64_value))
}

/// `str.trim_start()`: removes leading Unicode whitespace.
pub(crate) fn trim_start(text: &str) -> Value {
    Value::String(text.trim_start().to_string())
}

/// `str.trim_end()`: removes trailing Unicode whitespace.
pub(crate) fn trim_end(text: &str) -> Value {
    Value::String(text.trim_end().to_string())
}

/// `str.split_once(separator)` and `str.rsplit_once(separator)`.
///
/// Returns `(before, after) | None`. An empty separator traps with `AU4003`.
pub(crate) fn split_once(text: &str, separator: &str, from_end: bool) -> Result<Value> {
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
    let pair_type = Type::Tuple(vec![str_type(), str_type()]);
    Ok(optional_value(
        pair_type,
        split.map(|(before, after)| {
            Value::Tuple(TupleValue {
                element_types: vec![str_type(), str_type()],
                elements: vec![
                    Value::String(before.to_string()),
                    Value::String(after.to_string()),
                ],
            })
        }),
    ))
}

/// `str.lines()`: splits at `\n` and removes one `\r` before each `\n`.
/// A final line terminator adds no empty line, and an empty string has no
/// lines.
pub(crate) fn lines(text: &str) -> Value {
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.split_once('\n') {
            Some((line, after)) => {
                parts.push(line.strip_suffix('\r').unwrap_or(line).to_string());
                rest = after;
            }
            None => {
                parts.push(rest.to_string());
                break;
            }
        }
    }
    string_list(parts)
}

/// `str.split_whitespace()`: splits on runs of Unicode whitespace and drops
/// empty parts.
pub(crate) fn split_whitespace(text: &str) -> Value {
    string_list(text.split_whitespace().map(str::to_string))
}

/// `str.find(text)`: the index of the first occurrence, counted in Unicode
/// scalar values. An empty needle is found at index `0`.
pub(crate) fn find(text: &str, needle: &str) -> Value {
    optional_index(
        text.find(needle)
            .map(|byte_index| text[..byte_index].chars().count()),
    )
}

/// `str.count(text)`: the number of non-overlapping occurrences. An empty
/// needle traps with `AU4003`.
pub(crate) fn count_occurrences(text: &str, needle: &str) -> Result<Value> {
    if needle.is_empty() {
        return Err(Diagnostic::coded("AU4003", "`count` text cannot be empty"));
    }
    Ok(int64_value(text.matches(needle).count()))
}

/// `str.repeat(count)`: `count` concatenated copies. A negative count traps
/// with `AU4003`. A result above the string limit traps with `AU4005` before
/// any allocation.
pub(crate) fn repeat(text: &str, count: i64) -> Result<Value> {
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
    Ok(Value::String(output))
}

/// Byte search helpers for `bytes.starts_with`, `bytes.ends_with`, and
/// `bytes.find`.
pub(crate) fn is_bytes_search_builtin(name: &str) -> bool {
    matches!(
        name,
        "bytes::starts_with" | "bytes::ends_with" | "bytes::find"
    )
}

/// Copies one shared `list[uint8]` argument of a byte search builtin.
pub(crate) fn bytes_search_operand(name: &str, value: &Value) -> Result<Vec<u8>> {
    host_bytes_from_runtime(value, &name.replace("::", "."))
}

/// Evaluates one byte search builtin over two copied byte operands.
pub(crate) fn evaluate_bytes_search(name: &str, value: &[u8], pattern: &[u8]) -> Value {
    match name {
        "bytes::starts_with" => Value::Bool(value.starts_with(pattern)),
        "bytes::ends_with" => Value::Bool(value.ends_with(pattern)),
        _ => optional_index(memchr::memmem::find(value, pattern)),
    }
}
