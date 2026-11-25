//! The host part of the `regex` module.
//!
//! `std/regex.au` keeps a compiled pattern as a small number, its index in
//! a process-wide table of pattern texts, so a `regex.Regex` is one integer
//! that tasks can copy and share and a call passes without reading text out
//! of an object. Each thread keeps the compiled programs of the patterns it
//! used last and compiles a pattern again from its text when it has dropped
//! it. The table keeps every distinct pattern text for the life of the
//! process, and nothing else, up to [`MAX_PATTERN_COUNT`] patterns and
//! [`MAX_PATTERN_TABLE_BYTES`] bytes of text.

use super::*;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

/// The compiled patterns one thread keeps. A thread that uses more starts
/// again from an empty cache and compiles each pattern when it next runs.
/// A compiled program can take several megabytes, so the cache is small.
const REGEX_CACHE_PATTERNS: usize = 64;

/// The most distinct patterns a process keeps. Past it `regex.compile`
/// reports an error for a pattern it has not seen.
pub(crate) const MAX_PATTERN_COUNT: usize = 1 << 16;

/// The most bytes of distinct pattern text a process keeps.
pub(crate) const MAX_PATTERN_TABLE_BYTES: usize = 64 << 20;

/// The patterns a thread used last, found by comparing numbers without
/// hashing. Most programs use a handful of patterns in a loop.
const RECENT_PATTERNS: usize = 8;

/// Every pattern text `regex.compile` accepted, by its number.
#[derive(Default)]
struct PatternTable {
    texts: Vec<Arc<str>>,
    numbers: HashMap<Arc<str>, i64>,
    bytes: usize,
}

impl PatternTable {
    /// The number of `pattern`, adding it when the table has room for one
    /// more of at most `count` patterns and `bytes` bytes of text.
    fn number(&mut self, pattern: &str, count: usize, bytes: usize) -> std::result::Result<i64, String> {
        if let Some(number) = self.numbers.get(pattern) {
            return Ok(*number);
        }
        let total = self.bytes.saturating_add(pattern.len());
        if self.texts.len() >= count || total > bytes {
            return Err(format!(
                "the program has compiled more than {count} distinct patterns or {bytes} bytes of pattern text"
            ));
        }
        self.bytes = total;
        let text: Arc<str> = Arc::from(pattern);
        let number = self.texts.len() as i64;
        self.texts.push(Arc::clone(&text));
        self.numbers.insert(text, number);
        Ok(number)
    }
}

static PATTERNS: RwLock<Option<PatternTable>> = RwLock::new(None);

#[derive(Default)]
struct Compiled {
    recent: Vec<(i64, Rc<Regex>)>,
    replace_next: usize,
    all: HashMap<i64, Rc<Regex>>,
}

impl Compiled {
    fn remember(&mut self, number: i64, regex: &Rc<Regex>) {
        let entry = (number, Rc::clone(regex));
        if self.recent.len() < RECENT_PATTERNS {
            self.recent.push(entry);
        } else {
            self.recent[self.replace_next] = entry;
            self.replace_next = (self.replace_next + 1) % RECENT_PATTERNS;
        }
    }

    fn keep(&mut self, number: i64, regex: &Rc<Regex>) {
        if self.all.len() >= REGEX_CACHE_PATTERNS {
            self.all.clear();
        }
        self.all.insert(number, Rc::clone(regex));
        self.remember(number, regex);
    }
}

thread_local! {
    static COMPILED: RefCell<Compiled> = RefCell::new(Compiled::default());
}

/// Compiles `pattern`, or says why it is not a pattern.
fn compile(pattern: &str) -> std::result::Result<Rc<Regex>, String> {
    Regex::new(pattern)
        .map(Rc::new)
        .map_err(|error| match error {
            regex::Error::CompiledTooBig(limit) => {
                format!("the compiled pattern exceeds the {limit}-byte size limit")
            }
            other => other.to_string(),
        })
}

/// The number of `pattern`, which it keeps for the life of the process, or
/// the reason it is not a pattern.
fn pattern_number(pattern: &str) -> std::result::Result<i64, String> {
    let known = PATTERNS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|table| table.numbers.get(pattern).copied());
    if let Some(number) = known {
        return Ok(number);
    }
    let regex = compile(pattern)?;
    let number = {
        let mut table = PATTERNS
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        table
            .get_or_insert_with(PatternTable::default)
            .number(pattern, MAX_PATTERN_COUNT, MAX_PATTERN_TABLE_BYTES)?
    };
    COMPILED.with(|cache| cache.borrow_mut().keep(number, &regex));
    Ok(number)
}

/// The text of pattern `number`.
fn pattern_text(number: i64) -> Result<Arc<str>> {
    PATTERNS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|table| table.texts.get(usize::try_from(number).ok()?).cloned())
        .ok_or_else(|| {
            Diagnostic::coded(
                "AU4001",
                format!("`regex.Regex` holds no compiled pattern (number {number})"),
            )
        })
}

/// The compiled program of pattern `number`. The program is shared, not
/// cloned: a clone of a `Regex` starts with empty search scratch space and
/// would rebuild it on every call.
fn compiled(number: i64) -> Result<Rc<Regex>> {
    COMPILED.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, regex)) = cache.recent.iter().find(|(known, _)| *known == number) {
            return Ok(Rc::clone(regex));
        }
        if let Some(regex) = cache.all.get(&number).cloned() {
            cache.remember(number, &regex);
            return Ok(regex);
        }
        let text = pattern_text(number)?;
        let regex = compile(&text).map_err(|message| {
            Diagnostic::coded(
                "AU4001",
                format!("`regex.Regex` holds an invalid pattern: {message}"),
            )
        })?;
        cache.keep(number, &regex);
        Ok(regex)
    })
}

fn int64_list(values: Vec<i64>) -> Value {
    scalar_list("int64", values, ListElements::Int64)
}

/// A list of scalars in its scalar form. An empty list has no form yet.
fn scalar_list<T>(
    element_type: &str,
    values: Vec<T>,
    form: fn(SharedVec<T>) -> ListElements,
) -> Value {
    Value::Vec(VecValue {
        element_type: SharedType::named(element_type),
        elements: if values.is_empty() {
            ListElements::Values(Vec::new().into())
        } else {
            form(values.into())
        },
        str_view: Default::default(),
    })
}

fn str_list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Vec(VecValue {
        element_type: SharedType::named("str"),
        elements: ListElements::Values(values.into_iter().collect::<Vec<_>>().into()),
        str_view: Default::default(),
    })
}

/// Turns byte offsets into `text`, given in rising order, into offsets
/// counted in Unicode scalar values, as `str` indexes count.
struct ScalarOffsets<'a> {
    text: &'a str,
    ascii: bool,
    byte: usize,
    scalar: usize,
}

impl<'a> ScalarOffsets<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            ascii: text.is_ascii(),
            byte: 0,
            scalar: 0,
        }
    }

    fn at(&mut self, byte: usize) -> i64 {
        if self.ascii {
            return byte as i64;
        }
        self.scalar += self.text[self.byte..byte].chars().count();
        self.byte = byte;
        self.scalar as i64
    }
}

/// The third argument of a `regex` host call, when it has one.
pub enum RegexArgument<'a> {
    None,
    Text(&'a str),
}

/// The scalar searches of compiled code: 0 for whether the pattern matches
/// `text`, 1 for how many matches it has, and 2 for the index of the group
/// named `text`, or -1.
pub(crate) fn regex_scalar(operation: i64, number: i64, text: &str) -> Result<i64> {
    let regex = compiled(number)?;
    Ok(match operation {
        0 => i64::from(regex.is_match(text)),
        1 => regex.find_iter(text).count() as i64,
        _ => regex
            .capture_names()
            .position(|group| group == Some(text))
            .map_or(-1, |index| index as i64),
    })
}

/// Evaluates the `regex` host builtin `name` for pattern `number` over
/// `text`.
pub fn evaluate_regex_builtin(
    name: &str,
    number: i64,
    text: &str,
    argument: RegexArgument<'_>,
) -> Result<Value> {
    let regex = compiled(number)?;
    Ok(match name {
        "regex::__aura_is_match" => Value::Bool(regex.is_match(text)),
        "regex::__aura_count" => {
            Value::Int(IntegerValue::from_i64(regex.find_iter(text).count() as i64))
        }
        "regex::__aura_find" => {
            let int64 = || Type::named("int64");
            let found_type = || Type::Tuple(vec![Type::named("str"), int64(), int64()]);
            optional_kept(
                "(str, int64, int64)",
                &found_type,
                regex.find(text).map(|found| {
                    let mut offsets = ScalarOffsets::new(text);
                    let start = offsets.at(found.start());
                    let end = offsets.at(found.end());
                    Value::Tuple(TupleValue {
                        element_types: vec![Type::named("str"), int64(), int64()],
                        elements: vec![
                            Value::string(found.as_str()),
                            Value::Int(IntegerValue::from_i64(start)),
                            Value::Int(IntegerValue::from_i64(end)),
                        ],
                    })
                }),
            )
        }
        // The start and end of each match. `std/regex.au` slices the text.
        "regex::__aura_matches" => {
            let mut offsets = ScalarOffsets::new(text);
            let mut spans = Vec::new();
            for found in regex.find_iter(text) {
                spans.push(offsets.at(found.start()));
                spans.push(offsets.at(found.end()));
            }
            int64_list(spans)
        }
        "regex::__aura_find_all" => str_list(
            regex
                .find_iter(text)
                .map(|found| Value::string(found.as_str())),
        ),
        "regex::__aura_captures" => {
            // Each group's text, and whether the group took part in the
            // match. Both lists are empty when there is no match.
            let mut groups = Vec::new();
            let mut matched = Vec::new();
            if let Some(captures) = regex.captures(text) {
                groups.reserve(captures.len());
                matched.reserve(captures.len());
                for group in captures.iter() {
                    matched.push(crate::list_elements::BoolByte::from_bool(group.is_some()));
                    groups.push(Value::string(group.map_or("", |group| group.as_str())));
                }
            }
            let list_of = |item: &str| Type::Named("list".to_string(), vec![Type::named(item)]);
            Value::Tuple(TupleValue {
                element_types: vec![list_of("str"), list_of("bool")],
                elements: vec![
                    str_list(groups),
                    scalar_list("bool", matched, ListElements::Bool),
                ],
            })
        }
        // `text` is a group name here: its index, or -1 without one.
        "regex::__aura_group_index" => Value::Int(IntegerValue::from_i64(
            regex
                .capture_names()
                .position(|group| group == Some(text))
                .map_or(-1, |index| index as i64),
        )),
        "regex::__aura_split" => str_list(regex.split(text).map(Value::string)),
        "regex::__aura_replace" | "regex::__aura_replace_all" => {
            let RegexArgument::Text(replacement) = argument else {
                return Err(Diagnostic::coded(
                    "AU4001",
                    format!("`{name}` expects a replacement"),
                ));
            };
            let replaced =
                bounded_replace(&regex, text, replacement, name == "regex::__aura_replace_all")?;
            Value::string(replaced)
        }
        other => {
            return Err(Diagnostic::coded(
                "AU4001",
                format!("unknown regex host builtin `{other}`"),
            ))
        }
    })
}

fn replacement_too_long() -> Diagnostic {
    Diagnostic::coded(
        "AU4005",
        format!(
            "regex replacement result exceeds the maintained {MAX_STRING_BYTES}-byte string limit"
        ),
    )
}

/// Appends `piece` to `output` when the result stays inside the string
/// limit, reserving the room fallibly.
fn push_bounded(output: &mut String, piece: &str) -> Result<()> {
    if output.len().saturating_add(piece.len()) > MAX_STRING_BYTES {
        return Err(replacement_too_long());
    }
    output
        .try_reserve(piece.len())
        .map_err(|_| Diagnostic::coded("AU4005", "string allocation failed"))?;
    output.push_str(piece);
    Ok(())
}

/// A group reference at the start of `replacement`, which begins with
/// `$`: the group and the length of the reference. The syntax is the
/// `regex` crate's: `$name` takes the longest run of letters, digits and
/// `_`, `${name}` anything up to `}`, and a name of digits is a number.
fn group_reference(replacement: &str) -> Option<(&str, usize)> {
    let bytes = replacement.as_bytes();
    if bytes.get(1) == Some(&b'{') {
        let close = replacement[2..].find('}')?;
        return Some((&replacement[2..2 + close], close + 3));
    }
    let name_len = bytes[1..]
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
        .count();
    (name_len > 0).then(|| (&replacement[1..1 + name_len], name_len + 1))
}

/// `text` with its first match, or with every match when `all`, replaced
/// by `replacement` with its group references expanded. The result grows
/// piece by piece and stops at the string limit, so a large replacement
/// repeated over many matches is refused before it is built.
fn bounded_replace(regex: &Regex, text: &str, replacement: &str, all: bool) -> Result<String> {
    let mut output = String::new();
    let mut last = 0;
    let literal = !replacement.contains('$');
    for captures in regex.captures_iter(text) {
        let found = captures.get(0).expect("a match has group 0");
        push_bounded(&mut output, &text[last..found.start()])?;
        last = found.end();
        if literal {
            push_bounded(&mut output, replacement)?;
        } else {
            let mut rest = replacement;
            while let Some(at) = rest.find('$') {
                push_bounded(&mut output, &rest[..at])?;
                rest = &rest[at..];
                if rest.as_bytes().get(1) == Some(&b'$') {
                    push_bounded(&mut output, "$")?;
                    rest = &rest[2..];
                    continue;
                }
                let Some((group, length)) = group_reference(rest) else {
                    push_bounded(&mut output, "$")?;
                    rest = &rest[1..];
                    continue;
                };
                rest = &rest[length..];
                let value = match group.parse::<usize>() {
                    Ok(index) => captures.get(index),
                    Err(_) => captures.name(group),
                };
                push_bounded(&mut output, value.map_or("", |value| value.as_str()))?;
            }
            push_bounded(&mut output, rest)?;
        }
        if !all {
            break;
        }
    }
    push_bounded(&mut output, &text[last..])?;
    Ok(output)
}

/// `regex::__aura_compile`: the number of `pattern` and an empty text, or
/// -1 and the reason it is not a pattern.
pub fn host_regex_compile(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "regex::__aura_compile";
    host_expect_arity(name, &args, 1)?;
    let (number, message) = match pattern_number(host_string_ref_arg(&args, 0, name)?) {
        Ok(number) => (number, String::new()),
        Err(message) => (-1, message),
    };
    Ok(Value::Tuple(TupleValue {
        element_types: vec![Type::named("int64"), Type::named("str")],
        elements: vec![
            Value::Int(IntegerValue::from_i64(number)),
            Value::string(message),
        ],
    }))
}

/// `regex::__aura_pattern`: the text of pattern `number`.
pub fn host_regex_pattern(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "regex::__aura_pattern";
    host_expect_arity(name, &args, 1)?;
    let text = pattern_text(regex_number(&args[0], name)?)?;
    Ok(Value::string(&*text))
}

/// The pattern number a `regex` host call received as its first argument.
pub fn regex_number(value: &Value, name: &str) -> Result<i64> {
    match value {
        Value::Int(number) => number
            .as_i128()
            .and_then(|number| i64::try_from(number).ok()),
        _ => None,
    }
    .ok_or_else(|| {
        Diagnostic::coded(
            "AU4001",
            format!("`{name}` expects argument 1 to be a pattern number"),
        )
    })
}

/// `regex::__aura_escape`: `text` with every pattern metacharacter escaped,
/// so it matches itself.
pub fn host_regex_escape(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "regex::__aura_escape";
    host_expect_arity(name, &args, 1)?;
    let escaped = regex::escape(host_string_ref_arg(&args, 0, name)?);
    if escaped.len() > MAX_STRING_BYTES {
        return Err(Diagnostic::coded(
            "AU4005",
            format!("`regex.escape` result exceeds the maintained {MAX_STRING_BYTES}-byte string limit"),
        ));
    }
    Ok(Value::string(escaped))
}

fn host_regex_search(name: &str, args: Vec<Value>) -> Result<Value> {
    host_expect_arity(name, &args, 2)?;
    evaluate_regex_builtin(
        name,
        regex_number(&args[0], name)?,
        host_string_ref_arg(&args, 1, name)?,
        RegexArgument::None,
    )
}

fn host_regex_substitute(name: &str, args: Vec<Value>) -> Result<Value> {
    host_expect_arity(name, &args, 3)?;
    evaluate_regex_builtin(
        name,
        regex_number(&args[0], name)?,
        host_string_ref_arg(&args, 1, name)?,
        RegexArgument::Text(host_string_ref_arg(&args, 2, name)?),
    )
}

macro_rules! regex_host_functions {
    ($($function:ident => $kind:ident $name:literal;)*) => {
        $(
            pub fn $function(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
                $kind($name, args)
            }
        )*

        /// The interpreter's entry for a two- or three-text `regex` builtin.
        pub fn regex_host_function(name: &str) -> Option<host_builtins::HostBuiltinFn> {
            match name {
                $($name => Some($function),)*
                _ => None,
            }
        }
    };
}

regex_host_functions! {
    host_regex_is_match => host_regex_search "regex::__aura_is_match";
    host_regex_count => host_regex_search "regex::__aura_count";
    host_regex_find => host_regex_search "regex::__aura_find";
    host_regex_matches => host_regex_search "regex::__aura_matches";
    host_regex_find_all => host_regex_search "regex::__aura_find_all";
    host_regex_captures => host_regex_search "regex::__aura_captures";
    host_regex_group_index => host_regex_search "regex::__aura_group_index";
    host_regex_split => host_regex_search "regex::__aura_split";
    host_regex_replace => host_regex_substitute "regex::__aura_replace";
    host_regex_replace_all => host_regex_substitute "regex::__aura_replace_all";
}

#[cfg(test)]
#[path = "regex_host_tests.rs"]
mod tests;
