use std::collections::{HashMap, TryReserveError};
use std::fmt;
use std::io;

#[cfg(test)]
use std::cell::Cell;

use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::ser::{CompactFormatter, Formatter};

pub(crate) const MAX_JSON_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_JSON_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_JSON_DEPTH: usize = 128;
pub(crate) const MAX_JSON_INDENT: i64 = 16;
pub(crate) const MAX_JSON_VALUE_NODES: usize = 262_144;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum JsonCodecError {
    Syntax {
        message: String,
        line: usize,
        column: usize,
        offset: usize,
    },
    NumberOutOfRange {
        line: usize,
        column: usize,
        offset: usize,
    },
    NestingTooDeep {
        limit: usize,
        line: usize,
        column: usize,
        offset: usize,
    },
    InputTooLarge {
        actual_bytes: u64,
        limit_bytes: u64,
    },
    InvalidIndent {
        indent: i64,
        maximum: i64,
    },
    NonFiniteNumber,
    OutputTooLarge {
        limit_bytes: u64,
    },
    MaterializationTooLarge {
        limit: usize,
    },
    AllocationFailed,
}

impl fmt::Display for JsonCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax {
                message,
                line,
                column,
                ..
            } => write!(formatter, "JSON syntax error at {line}:{column}: {message}"),
            Self::NumberOutOfRange { line, column, .. } => {
                write!(
                    formatter,
                    "JSON number at {line}:{column} is outside float64 range"
                )
            }
            Self::NestingTooDeep {
                limit,
                line,
                column,
                ..
            } if *line > 0 => write!(
                formatter,
                "JSON nesting at {line}:{column} exceeds the maximum depth of {limit}"
            ),
            Self::NestingTooDeep { limit, .. } => {
                write!(formatter, "JSON value exceeds the maximum depth of {limit}")
            }
            Self::InputTooLarge {
                actual_bytes,
                limit_bytes,
            } => write!(
                formatter,
                "JSON input is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit"
            ),
            Self::InvalidIndent { indent, maximum } => write!(
                formatter,
                "JSON indent must be between 0 and {maximum}, found {indent}"
            ),
            Self::NonFiniteNumber => {
                formatter.write_str("JSON cannot encode NaN or an infinite float")
            }
            Self::OutputTooLarge { limit_bytes } => {
                write!(
                    formatter,
                    "JSON output exceeds the {limit_bytes}-byte limit"
                )
            }
            Self::MaterializationTooLarge { limit } => {
                write!(
                    formatter,
                    "JSON value exceeds the maximum materialized node count of {limit}"
                )
            }
            Self::AllocationFailed => {
                formatter.write_str("memory allocation failed while processing JSON")
            }
        }
    }
}

impl std::error::Error for JsonCodecError {}

pub(crate) fn check_input_len(length: usize) -> Result<(), JsonCodecError> {
    if length <= MAX_JSON_INPUT_BYTES {
        Ok(())
    } else {
        Err(JsonCodecError::InputTooLarge {
            actual_bytes: length as u64,
            limit_bytes: MAX_JSON_INPUT_BYTES as u64,
        })
    }
}

pub(crate) fn check_output_len(current: usize, added: usize) -> Result<(), JsonCodecError> {
    match current.checked_add(added) {
        Some(total) if total <= MAX_JSON_OUTPUT_BYTES => Ok(()),
        _ => Err(JsonCodecError::OutputTooLarge {
            limit_bytes: MAX_JSON_OUTPUT_BYTES as u64,
        }),
    }
}

pub(crate) fn parse(source: &str) -> Result<JsonValue, JsonCodecError> {
    check_input_len(source.len())?;
    validate_source_depth(source)?;
    validate_syntax(source)?;
    validate_number_ranges(source)?;

    let mut deserializer = serde_json::Deserializer::from_str(source);
    deserializer.disable_recursion_limit();
    JsonValue::deserialize(&mut deserializer)
        .and_then(|value| {
            deserializer.end()?;
            Ok(value)
        })
        .map_err(|error| deserialize_error(source, error))
}

pub(crate) fn dumps(value: &JsonValue, indent: Option<i64>) -> Result<String, JsonCodecError> {
    if let Some(indent) = indent {
        if !(0..=MAX_JSON_INDENT).contains(&indent) {
            return Err(JsonCodecError::InvalidIndent {
                indent,
                maximum: MAX_JSON_INDENT,
            });
        }
    }

    let mut output = BoundedOutput::default();
    write_value(&mut output, value, indent.map(|value| value as usize), 0)?;
    Ok(output.text)
}

#[cfg(test)]
impl JsonValue {
    pub(crate) fn object(entries: Vec<(String, JsonValue)>) -> Self {
        let mut unique = ObjectBuilder::try_with_capacity(entries.len())
            .expect("test JSON object allocation should succeed");
        for (key, value) in entries {
            unique
                .insert(key, value)
                .expect("test JSON object allocation should succeed");
        }
        Self::Object(unique.finish())
    }
}

fn validate_source_depth(source: &str) -> Result<(), JsonCodecError> {
    let bytes = source.as_bytes();
    let mut index = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        match byte {
            b'"' => {
                in_string = true;
                index += 1;
            }
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    let (line, column) = line_column_for_offset(source, index);
                    return Err(JsonCodecError::NestingTooDeep {
                        limit: MAX_JSON_DEPTH,
                        line,
                        column,
                        offset: index,
                    });
                }
                index += 1;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }
    Ok(())
}

fn validate_syntax(source: &str) -> Result<(), JsonCodecError> {
    let mut deserializer = serde_json::Deserializer::from_str(source);
    deserializer.disable_recursion_limit();
    IgnoredAny::deserialize(&mut deserializer)
        .and_then(|_| deserializer.end())
        .map_err(|error| syntax_error(source, error))
}

fn validate_number_ranges(source: &str) -> Result<(), JsonCodecError> {
    let bytes = source.as_bytes();
    let mut index = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        match byte {
            b'"' => {
                in_string = true;
                index += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = index;
                while index < bytes.len()
                    && matches!(bytes[index], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    index += 1;
                }
                let lexeme = &source[start..index];
                if classify_number(lexeme).is_none() {
                    let (line, column) = line_column_for_offset(source, start);
                    return Err(JsonCodecError::NumberOutOfRange {
                        line,
                        column,
                        offset: start,
                    });
                }
            }
            _ => index += 1,
        }
    }
    Ok(())
}

fn advance_byte_position(byte: u8, line: &mut usize, column: &mut usize) {
    if byte == b'\n' {
        *line += 1;
        *column = 1;
    } else {
        *column += 1;
    }
}

fn classify_number(lexeme: &str) -> Option<JsonValue> {
    if let Some(value) = exact_integral_i64(lexeme) {
        return Some(JsonValue::Int(value));
    }
    lexeme
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(JsonValue::Float)
}

fn exact_integral_i64(lexeme: &str) -> Option<i64> {
    let (negative, unsigned) = match lexeme.strip_prefix('-') {
        Some(unsigned) => (true, unsigned),
        None => (false, lexeme),
    };
    let (mantissa, exponent_text) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, None), |(mantissa, exponent)| {
            (mantissa, Some(exponent))
        });
    let (integer, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, ""), |(integer, fraction)| (integer, fraction));

    let digits = || integer.bytes().chain(fraction.bytes());
    if digits().all(|digit| digit == b'0') {
        return Some(0);
    }

    let exponent = exponent_text.map_or(0, |text| {
        text.parse::<i128>().unwrap_or_else(|_| {
            if text.starts_with('-') {
                i128::MIN / 2
            } else {
                i128::MAX / 2
            }
        })
    });
    let scale = exponent.saturating_sub(fraction.len() as i128);
    let total_digits = integer.len().checked_add(fraction.len())?;

    let mut magnitude = 0i128;
    if scale < 0 {
        let removed = usize::try_from(scale.saturating_neg()).ok()?;
        if removed > total_digits {
            return None;
        }
        let kept = total_digits - removed;
        if digits().skip(kept).any(|digit| digit != b'0') {
            return None;
        }
        for digit in digits().take(kept) {
            magnitude = magnitude
                .checked_mul(10)?
                .checked_add(i128::from(digit - b'0'))?;
        }
    } else {
        for digit in digits() {
            magnitude = magnitude
                .checked_mul(10)?
                .checked_add(i128::from(digit - b'0'))?;
        }
        let appended_zeroes = u32::try_from(scale).ok()?;
        magnitude = magnitude.checked_mul(10i128.checked_pow(appended_zeroes)?)?;
    }

    let signed = if negative {
        magnitude.checked_neg()?
    } else {
        magnitude
    };
    i64::try_from(signed).ok()
}

const JSON_ALLOCATION_FAILED_SENTINEL: &str = "__aurora_json_allocation_failed__";
const JSON_MATERIALIZATION_TOO_LARGE_SENTINEL: &str = "__aurora_json_materialization_too_large__";

fn deserialize_error(source: &str, error: serde_json::Error) -> JsonCodecError {
    let rendered = error.to_string();
    if rendered.starts_with(JSON_ALLOCATION_FAILED_SENTINEL) {
        JsonCodecError::AllocationFailed
    } else if rendered.starts_with(JSON_MATERIALIZATION_TOO_LARGE_SENTINEL) {
        JsonCodecError::MaterializationTooLarge {
            limit: MAX_JSON_VALUE_NODES,
        }
    } else {
        syntax_error(source, error)
    }
}

fn syntax_error(source: &str, error: serde_json::Error) -> JsonCodecError {
    let serde_line = error.line();
    let serde_column = error.column();
    let offset = if error.is_eof() {
        source.len()
    } else {
        byte_offset_for_line_column(source, serde_line, serde_column)
    };
    let (line, column) = line_column_for_offset(source, offset);
    let rendered = error.to_string();
    let location_suffix = format!(" at line {serde_line} column {serde_column}");
    let message = rendered
        .strip_suffix(&location_suffix)
        .unwrap_or(&rendered)
        .to_owned();
    JsonCodecError::Syntax {
        message,
        line,
        column,
        offset,
    }
}

fn byte_offset_for_line_column(source: &str, target_line: usize, target_column: usize) -> usize {
    let mut line = 1usize;
    let mut column = 1usize;
    for (offset, byte) in source.bytes().enumerate() {
        if line == target_line && column == target_column {
            return offset;
        }
        advance_byte_position(byte, &mut line, &mut column);
    }
    source.len()
}

fn line_column_for_offset(source: &str, target_offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (offset, character) in source.char_indices() {
        if offset >= target_offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn try_vec_with_capacity<T>(capacity: usize) -> Result<Vec<T>, TryReserveError> {
    let mut values = Vec::new();
    values.try_reserve(capacity)?;
    Ok(values)
}

fn try_push<T>(values: &mut Vec<T>, value: T) -> Result<(), TryReserveError> {
    values.try_reserve(1)?;
    values.push(value);
    Ok(())
}

fn try_owned_string(value: &str) -> Result<String, TryReserveError> {
    try_owned_string_with_capacity(value, value.len())
}

fn try_owned_string_with_capacity(value: &str, capacity: usize) -> Result<String, TryReserveError> {
    let mut owned = String::new();
    owned.try_reserve(capacity.max(value.len()))?;
    owned.push_str(value);
    Ok(owned)
}

macro_rules! try_serde_allocation {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(_) => return Err(de::Error::custom(JSON_ALLOCATION_FAILED_SENTINEL)),
        }
    };
}

struct JsonMaterializationBudget {
    remaining: usize,
}

impl JsonMaterializationBudget {
    fn new() -> Self {
        Self {
            remaining: MAX_JSON_VALUE_NODES,
        }
    }

    fn claim<E: de::Error>(&mut self) -> Result<(), E> {
        if self.remaining == 0 {
            Err(E::custom(JSON_MATERIALIZATION_TOO_LARGE_SENTINEL))
        } else {
            self.remaining -= 1;
            Ok(())
        }
    }
}

struct JsonValueSeed<'a> {
    budget: &'a mut JsonMaterializationBudget,
}

impl<'de> DeserializeSeed<'de> for JsonValueSeed<'_> {
    type Value = JsonValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.budget.claim::<D::Error>()?;
        deserializer.deserialize_any(JsonValueVisitor {
            budget: self.budget,
        })
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut budget = JsonMaterializationBudget::new();
        JsonValueSeed {
            budget: &mut budget,
        }
        .deserialize(deserializer)
    }
}

struct JsonValueVisitor<'a> {
    budget: &'a mut JsonMaterializationBudget,
}

impl<'de> Visitor<'de> for JsonValueVisitor<'_> {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    // This codec only deserializes from serde_json's StrRead pipeline:
    // arbitrary-precision decimal/float lexemes arrive through `visit_map`,
    // strings through `visit_str`, and JSON null through `visit_unit`.
    // The optional `visit_f64`, `visit_string`, and `visit_none` callbacks
    // therefore are intentionally not part of this visitor's surface.

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(JsonValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(JsonValue::Int(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        i64::try_from(value)
            .map(JsonValue::Int)
            .or_else(|_| Ok(JsonValue::Float(value as f64)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(JsonValue::String(try_serde_allocation!(try_owned_string(
            value
        ))))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(JsonValue::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let initial_capacity = sequence.size_hint().unwrap_or(0).min(self.budget.remaining);
        let mut values = try_serde_allocation!(try_vec_with_capacity(initial_capacity));
        while let Some(value) = sequence.next_element_seed(JsonValueSeed {
            budget: &mut *self.budget,
        })? {
            try_serde_allocation!(try_push(&mut values, value));
        }
        Ok(JsonValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let Some(first_key) = map.next_key::<JsonMapKey>()? else {
            return Ok(JsonValue::Object(Vec::new()));
        };

        match first_key {
            JsonMapKey::ArbitraryPrecisionNumber => {
                let lexeme = map.next_value::<FallibleString>()?;
                classify_number(&lexeme.0)
                    .ok_or_else(|| de::Error::custom("JSON number is outside float64 range"))
            }
            JsonMapKey::Object(first_key) => {
                let capacity = map
                    .size_hint()
                    .unwrap_or(0)
                    .saturating_add(1)
                    .min(self.budget.remaining);
                let mut entries = try_serde_allocation!(ObjectBuilder::try_with_capacity(capacity));
                try_serde_allocation!(entries.insert(
                    first_key,
                    map.next_value_seed(JsonValueSeed {
                        budget: &mut *self.budget,
                    })?,
                ));
                while let Some(key) = map.next_key::<FallibleString>()? {
                    try_serde_allocation!(entries.insert(
                        key.0,
                        map.next_value_seed(JsonValueSeed {
                            budget: &mut *self.budget,
                        })?,
                    ));
                }
                Ok(JsonValue::Object(entries.finish()))
            }
        }
    }
}

const ARBITRARY_PRECISION_NUMBER_KEY: &str = "$serde_json::private::Number";

struct FallibleString(String);

impl<'de> Deserialize<'de> for FallibleString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_string(FallibleStringVisitor)
    }
}

struct FallibleStringVisitor;

impl<'de> Visitor<'de> for FallibleStringVisitor {
    type Value = FallibleString;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON string")
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(value)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(FallibleString(try_serde_allocation!(try_owned_string(
            value
        ))))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(FallibleString(value))
    }
}

enum JsonMapKey {
    ArbitraryPrecisionNumber,
    Object(String),
}

impl<'de> Deserialize<'de> for JsonMapKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // serde_json presents an arbitrary-precision number as a synthetic
        // one-entry map. Its synthetic key forwards `deserialize_option`
        // directly to a string visitor, while a real JSON object key enters
        // through `visit_some`. Probe that distinction before looking at the
        // key text so a user key equal to serde_json's marker stays ordinary
        // object data.
        deserializer.deserialize_option(JsonMapKeyVisitor)
    }
}

struct JsonMapKeyVisitor;

impl<'de> Visitor<'de> for JsonMapKeyVisitor {
    type Value = JsonMapKey;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object key or arbitrary-precision number marker")
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        FallibleString::deserialize(deserializer).map(|key| JsonMapKey::Object(key.0))
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(value)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value == ARBITRARY_PRECISION_NUMBER_KEY {
            Ok(JsonMapKey::ArbitraryPrecisionNumber)
        } else {
            Err(E::custom(
                "invalid serde_json arbitrary-precision number marker",
            ))
        }
    }
}

struct ObjectBuilder {
    entries: Vec<(String, JsonValue)>,
    entry_indexes: HashMap<String, usize>,
}

impl ObjectBuilder {
    fn try_with_capacity(capacity: usize) -> Result<Self, TryReserveError> {
        let entries = try_vec_with_capacity(capacity)?;
        let mut entry_indexes = HashMap::new();
        entry_indexes.try_reserve(capacity)?;
        Ok(Self {
            entries,
            entry_indexes,
        })
    }

    fn insert(&mut self, key: String, value: JsonValue) -> Result<(), TryReserveError> {
        record_object_key_probe();
        if let Some(index) = self.entry_indexes.get(key.as_str()).copied() {
            self.entries[index].1 = value;
            return Ok(());
        }

        self.entries.try_reserve(1)?;
        self.entry_indexes.try_reserve(1)?;
        let index_key = try_owned_string(&key)?;

        let entry_index = self.entries.len();
        self.entries.push((key, value));
        self.entry_indexes.insert(index_key, entry_index);
        Ok(())
    }

    fn finish(self) -> Vec<(String, JsonValue)> {
        self.entries
    }
}

#[cfg(test)]
thread_local! {
    static OBJECT_KEY_PROBE_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn record_object_key_probe() {
    OBJECT_KEY_PROBE_COUNT.with(|count| count.set(count.get() + 1));
}

#[cfg(not(test))]
fn record_object_key_probe() {}

#[cfg(test)]
fn reset_object_key_probe_count() {
    OBJECT_KEY_PROBE_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn object_key_probe_count() -> usize {
    OBJECT_KEY_PROBE_COUNT.with(Cell::get)
}

#[derive(Default)]
struct BoundedOutput {
    text: String,
}

impl BoundedOutput {
    fn push_str(&mut self, text: &str) -> Result<(), JsonCodecError> {
        check_output_len(self.text.len(), text.len())?;
        self.text
            .try_reserve(text.len())
            .map_err(allocation_error)?;
        self.text.push_str(text);
        Ok(())
    }
}

fn allocation_error(_: std::collections::TryReserveError) -> JsonCodecError {
    JsonCodecError::AllocationFailed
}

struct BoundedIoWriter<'a> {
    output: &'a mut BoundedOutput,
    failure: Option<JsonCodecError>,
}

impl io::Write for BoundedIoWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // CompactFormatter's numeric methods only emit ASCII.
        let text = std::str::from_utf8(bytes).expect("JSON numeric formatter emits valid UTF-8");
        match self.output.push_str(text) {
            Ok(()) => Ok(bytes.len()),
            Err(error) => {
                self.failure = Some(error);
                Err(io::ErrorKind::Other.into())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_int(output: &mut BoundedOutput, value: i64) -> Result<(), JsonCodecError> {
    let mut writer = BoundedIoWriter {
        output,
        failure: None,
    };
    let result = CompactFormatter.write_i64(&mut writer, value);
    finish_number_write(writer, result)
}

fn write_float(output: &mut BoundedOutput, value: f64) -> Result<(), JsonCodecError> {
    let mut writer = BoundedIoWriter {
        output,
        failure: None,
    };
    let result = CompactFormatter.write_f64(&mut writer, value);
    finish_number_write(writer, result)
}

fn finish_number_write(
    writer: BoundedIoWriter<'_>,
    result: io::Result<()>,
) -> Result<(), JsonCodecError> {
    match result {
        Ok(()) => Ok(()),
        Err(_) => Err(writer
            .failure
            .expect("bounded numeric writer records every I/O failure")),
    }
}

fn write_value(
    output: &mut BoundedOutput,
    value: &JsonValue,
    indent: Option<usize>,
    root_depth: usize,
) -> Result<(), JsonCodecError> {
    enum WriteFrame<'a> {
        Array {
            values: &'a [JsonValue],
            next_index: usize,
            depth: usize,
            child_depth: usize,
        },
        Object {
            sorted: Vec<SortedObjectEntry<'a>>,
            next_sorted_index: usize,
            depth: usize,
            child_depth: usize,
        },
    }

    let mut frames = Vec::new();
    let mut next = Some((value, root_depth));

    loop {
        if let Some((value, depth)) = next.take() {
            match value {
                JsonValue::Null => output.push_str("null")?,
                JsonValue::Bool(value) => output.push_str(if *value { "true" } else { "false" })?,
                JsonValue::Int(value) => write_int(output, *value)?,
                JsonValue::Float(value) if value.is_finite() => write_float(output, *value)?,
                JsonValue::Float(_) => return Err(JsonCodecError::NonFiniteNumber),
                JsonValue::String(value) => write_string(output, value)?,
                JsonValue::Array(values) => {
                    let child_depth = checked_dump_depth(depth)?;
                    output.push_str("[")?;
                    if let Some(value) = values.first() {
                        write_item_prefix(output, indent, child_depth)?;
                        frames.try_reserve(1).map_err(allocation_error)?;
                        frames.push(WriteFrame::Array {
                            values,
                            next_index: 1,
                            depth,
                            child_depth,
                        });
                        next = Some((value, child_depth));
                    } else {
                        write_container_suffix(output, indent, depth, false)?;
                        output.push_str("]")?;
                    }
                }
                JsonValue::Object(entries) => {
                    let child_depth = checked_dump_depth(depth)?;
                    let sorted = sorted_object_entries_with_capacity(entries, entries.len())?;
                    output.push_str("{")?;
                    if sorted.is_empty() {
                        write_container_suffix(output, indent, depth, false)?;
                        output.push_str("}")?;
                    } else {
                        let mut last_duplicate = 0;
                        let key = sorted[last_duplicate].1;
                        while last_duplicate + 1 < sorted.len()
                            && sorted[last_duplicate + 1].1 == key
                        {
                            last_duplicate += 1;
                        }
                        let value = sorted[last_duplicate].2;
                        write_item_prefix(output, indent, child_depth)?;
                        write_string(output, key)?;
                        output.push_str(if indent.is_some() { ": " } else { ":" })?;
                        frames.try_reserve(1).map_err(allocation_error)?;
                        frames.push(WriteFrame::Object {
                            sorted,
                            next_sorted_index: last_duplicate + 1,
                            depth,
                            child_depth,
                        });
                        next = Some((value, child_depth));
                    }
                }
            }
        } else {
            let frame = frames
                .pop()
                .expect("iterative JSON writer always has a pending frame");
            match frame {
                WriteFrame::Array {
                    values,
                    next_index,
                    depth,
                    child_depth,
                } => {
                    if let Some(value) = values.get(next_index) {
                        output.push_str(",")?;
                        write_item_prefix(output, indent, child_depth)?;
                        frames.try_reserve(1).map_err(allocation_error)?;
                        frames.push(WriteFrame::Array {
                            values,
                            next_index: next_index + 1,
                            depth,
                            child_depth,
                        });
                        next = Some((value, child_depth));
                    } else {
                        write_container_suffix(output, indent, depth, !values.is_empty())?;
                        output.push_str("]")?;
                    }
                }
                WriteFrame::Object {
                    sorted,
                    next_sorted_index,
                    depth,
                    child_depth,
                } => {
                    if next_sorted_index < sorted.len() {
                        let mut last_duplicate = next_sorted_index;
                        let key = sorted[last_duplicate].1;
                        while last_duplicate + 1 < sorted.len()
                            && sorted[last_duplicate + 1].1 == key
                        {
                            last_duplicate += 1;
                        }
                        let value = sorted[last_duplicate].2;
                        output.push_str(",")?;
                        write_item_prefix(output, indent, child_depth)?;
                        write_string(output, key)?;
                        output.push_str(if indent.is_some() { ": " } else { ":" })?;
                        frames.try_reserve(1).map_err(allocation_error)?;
                        frames.push(WriteFrame::Object {
                            sorted,
                            next_sorted_index: last_duplicate + 1,
                            depth,
                            child_depth,
                        });
                        next = Some((value, child_depth));
                    } else {
                        write_container_suffix(output, indent, depth, !sorted.is_empty())?;
                        output.push_str("}")?;
                    }
                }
            }
        }

        if next.is_none() && frames.is_empty() {
            return Ok(());
        }
    }
}

type SortedObjectEntry<'a> = (usize, &'a str, &'a JsonValue);

fn sorted_object_entries_with_capacity<'a>(
    entries: &'a [(String, JsonValue)],
    capacity: usize,
) -> Result<Vec<SortedObjectEntry<'a>>, JsonCodecError> {
    let mut sorted = Vec::new();
    sorted
        .try_reserve(capacity.max(entries.len()))
        .map_err(allocation_error)?;
    sorted.extend(
        entries
            .iter()
            .enumerate()
            .map(|(index, (key, value))| (index, key.as_str(), value)),
    );
    sorted.sort_unstable_by(|left, right| left.1.cmp(right.1).then_with(|| left.0.cmp(&right.0)));
    Ok(sorted)
}

fn checked_dump_depth(depth: usize) -> Result<usize, JsonCodecError> {
    let child_depth = depth + 1;
    if child_depth > MAX_JSON_DEPTH {
        Err(JsonCodecError::NestingTooDeep {
            limit: MAX_JSON_DEPTH,
            line: 0,
            column: 0,
            offset: 0,
        })
    } else {
        Ok(child_depth)
    }
}

fn write_item_prefix(
    output: &mut BoundedOutput,
    indent: Option<usize>,
    depth: usize,
) -> Result<(), JsonCodecError> {
    if let Some(indent) = indent {
        output.push_str("\n")?;
        write_spaces(output, indent.saturating_mul(depth))?;
    }
    Ok(())
}

fn write_container_suffix(
    output: &mut BoundedOutput,
    indent: Option<usize>,
    depth: usize,
    nonempty: bool,
) -> Result<(), JsonCodecError> {
    if nonempty {
        if let Some(indent) = indent {
            output.push_str("\n")?;
            write_spaces(output, indent.saturating_mul(depth))?;
        }
    }
    Ok(())
}

fn write_spaces(output: &mut BoundedOutput, count: usize) -> Result<(), JsonCodecError> {
    const SPACES: &str = "                ";
    let mut remaining = count;
    while remaining > 0 {
        let next = remaining.min(SPACES.len());
        output.push_str(&SPACES[..next])?;
        remaining -= next;
    }
    Ok(())
}

fn write_string(output: &mut BoundedOutput, value: &str) -> Result<(), JsonCodecError> {
    output.push_str("\"")?;
    let mut unescaped_start = 0usize;
    for (offset, character) in value.char_indices() {
        let escaped = match character {
            '"' => Some("\\\""),
            '\\' => Some("\\\\"),
            '\u{08}' => Some("\\b"),
            '\u{0c}' => Some("\\f"),
            '\n' => Some("\\n"),
            '\r' => Some("\\r"),
            '\t' => Some("\\t"),
            character if character <= '\u{1f}' => {
                if unescaped_start < offset {
                    output.push_str(&value[unescaped_start..offset])?;
                }
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = character as usize;
                let escaped = [b'\\', b'u', b'0', b'0', HEX[code >> 4], HEX[code & 0x0f]];
                // Every byte in the escape is ASCII.
                output
                    .push_str(std::str::from_utf8(&escaped).expect("JSON escape is valid UTF-8"))?;
                unescaped_start = offset + character.len_utf8();
                continue;
            }
            _ => None,
        };

        let Some(escaped) = escaped else {
            continue;
        };
        if unescaped_start < offset {
            output.push_str(&value[unescaped_start..offset])?;
        }
        output.push_str(escaped)?;
        unescaped_start = offset + character.len_utf8();
    }
    if unescaped_start < value.len() {
        output.push_str(&value[unescaped_start..])?;
    }
    output.push_str("\"")
}

#[cfg(test)]
#[path = "json_codec_tests.rs"]
mod tests;
