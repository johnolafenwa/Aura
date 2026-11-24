//! Models of `str` operations.
//!
//! A `str` is an SMT string: a sequence of Unicode scalar values, as `len`
//! and every index count them. The run-time methods search bytes of valid
//! UTF-8, where a match always starts and ends between scalar values, so a
//! search over scalar values finds the same matches.
//!
//! An operation whose operands are all literals is computed here with the
//! library functions the runtime calls. Otherwise the result is an SMT term
//! when the strings theory has the operation with the same meaning:
//! `str.at`, `str.substr`, `str.indexof`. Such a term is
//! used in place and not named by a fact, so only a check that reads the
//! result gives the solver the operation: a fact with `str.substr` at a
//! large offset slows every later check of the function, and several have
//! kept Z3 past its time limit.
//!
//! Any other result is a made-up constant with the facts that hold of
//! every result. Such a constant is recorded as not modeled, so a check the
//! facts do not decide is not reported with an invented counterexample.
//! `replace` is one of these: the solvers decide too little about
//! `str.replace_all` for it to be worth its cost. The facts about such a
//! constant are given only to the checks that read it: a fact about a
//! string, asserted for the whole function, slows every check the solver
//! refutes after it, also the checks that read no string.
//!
//! The parts of `split`, `lines`, `split_whitespace`, and `chars` are a
//! list. What holds of each part is assumed where the function reads the
//! part, which keeps the facts free of quantifiers.

use std::collections::{BTreeMap, BTreeSet};

use super::exec::{is_str, smt_string, symbols, Exec, Goal, IntTy, Obligation, State, Store, Val};
use super::ObligationKind;
use crate::ast::{Expr, FormatPart};
use crate::diag::Span;
use crate::mir::MirFormatPart;
use crate::runtime_ops::{RuntimeOp, StrOp};
use crate::sema::{optional_type, Type};

/// The most parts a list computed from literals may have.
const MAX_PARTS: usize = 32;
/// The most copies `repeat` or parts `join` writes out as one term.
const MAX_COPIES: i128 = 8;
/// The longest f-string term used in place, in bytes.
const MAX_INLINE: usize = 400;

/// A condition the runtime checks before an operation, and what it is
/// called in a report.
type Need = (String, &'static str);

/// What holds of each element of a list a `str` method returned.
#[derive(Clone, Debug)]
enum Parts {
    /// Element `i` is character `i` of the text.
    Chars(String),
    /// Each element is a part of the text that holds none of `without`,
    /// and has at least one character when `filled`.
    Inside {
        text: String,
        without: Vec<String>,
        filled: bool,
    },
}

/// What the models remember across one function.
#[derive(Default)]
pub(super) struct TextState {
    /// The result of each operation by its operands: the same operation on
    /// the same values gives the same result.
    models: BTreeMap<String, (Val, Vec<Need>)>,
    /// The element stores of the lists the models returned.
    parts: BTreeMap<String, Parts>,
    /// The element reads whose facts are assumed already.
    read: BTreeSet<(String, String)>,
    /// The made-up decimal text of each integer term.
    digits: BTreeMap<String, String>,
    /// The facts that say what a made-up result is, by the result's
    /// constant. Such a fact holds of some result whatever the operands
    /// are, so it says nothing about them.
    described: BTreeMap<usize, String>,
}

impl TextState {
    /// Whether fact `index` only says what a made-up result other than
    /// `symbol` is.
    pub(super) fn describes_other(&self, index: usize, symbol: &str) -> bool {
        self.described
            .get(&index)
            .is_some_and(|result| result != symbol)
    }
}

/// A value computed from literal operands.
enum Folded {
    Text(String),
    Count(usize),
    List(Vec<String>),
}

fn str_type() -> Type {
    Type::named("str")
}

/// The text of an SMT string literal that `smt_string` wrote.
fn literal(term: &str) -> Option<String> {
    let body = term.strip_prefix('"')?.strip_suffix('"')?;
    let mut text = String::new();
    let mut rest = body;
    while let Some(character) = rest.chars().next() {
        if let Some(escape) = rest.strip_prefix("\\u{") {
            let (code, after) = escape.split_once('}')?;
            text.push(char::from_u32(u32::from_str_radix(code, 16).ok()?)?);
            rest = after;
        } else {
            text.push(character);
            rest = &rest[character.len_utf8()..];
            // A quote inside a literal is written twice.
            if character == '"' {
                rest = rest.strip_prefix('"')?;
            }
        }
    }
    Some(text)
}

/// The value of an SMT integer literal.
fn int_literal(term: &str) -> Option<i128> {
    match term
        .strip_prefix("(- ")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        Some(rest) => rest.parse::<i128>().ok()?.checked_neg(),
        None => term.parse().ok(),
    }
}

/// Whether a term names a variable a quantifier binds. No declared
/// constant may be defined by such a term.
fn is_bound(term: &str) -> bool {
    term.contains("!q")
}

fn len_of(text: &str) -> String {
    format!("(str.len {text})")
}

/// `index` as the runtime reads it in a text of `len` characters: a
/// negative index counts from the end.
fn from_end(len: &str, index: &str) -> String {
    match int_literal(index) {
        Some(0..) => index.to_string(),
        _ => format!("(ite (< {index} 0) (+ {index} {len}) {index})"),
    }
}

/// The lines of `text`, as the runtime's `lines` splits them: at each line
/// feed, without the carriage return before it, and with no empty line
/// after a final line feed.
fn lines(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (line, after) = rest.split_once('\n').unwrap_or((rest, ""));
        let ended = line.len() < rest.len();
        parts.push(match line.strip_suffix('\r') {
            Some(line) if ended => line.to_string(),
            _ => line.to_string(),
        });
        rest = after;
    }
    parts
}

/// The result of `op` on literal operands, as the runtime computes it.
fn fold(op: StrOp, text: &str, words: &[String], count: Option<i128>) -> Option<Folded> {
    let word = || words.first().map(String::as_str);
    Some(match op {
        StrOp::Split => Folded::List(text.split(word()?).map(str::to_string).collect()),
        StrOp::Lines => Folded::List(lines(text)),
        StrOp::SplitWhitespace => {
            Folded::List(text.split_whitespace().map(str::to_string).collect())
        }
        StrOp::Chars => Folded::List(text.chars().map(String::from).collect()),
        StrOp::Trim => Folded::Text(text.trim().to_string()),
        StrOp::TrimStart => Folded::Text(text.trim_start().to_string()),
        StrOp::TrimEnd => Folded::Text(text.trim_end().to_string()),
        StrOp::ToLower => Folded::Text(text.to_lowercase()),
        StrOp::ToUpper => Folded::Text(text.to_uppercase()),
        StrOp::Replace => Folded::Text(text.replace(word()?, words.get(1)?)),
        StrOp::Repeat => {
            let count = usize::try_from(count?).ok().filter(|count| *count <= 64)?;
            Folded::Text(text.repeat(count))
        }
        StrOp::ByteLen => Folded::Count(text.len()),
        // An empty text to count stops the program.
        StrOp::Count => Folded::Count(
            text.matches(word().filter(|word| !word.is_empty())?)
                .count(),
        ),
        _ => return None,
    })
}

impl Exec<'_> {
    /// The text `to_string`, `str`, and an f-string write for a scalar: a
    /// `str` as it is, an integer in decimal, a `bool` as `true` or `false`.
    fn display(&mut self, value: &Val) -> Option<String> {
        match value {
            Val::Str(term) => Some(term.clone()),
            Val::Int(term, ty) => self.decimal(term, *ty),
            Val::Bool(term) => Some(match term.as_str() {
                "true" => "\"true\"".to_string(),
                "false" => "\"false\"".to_string(),
                _ => format!("(ite {term} \"true\" \"false\")"),
            }),
            _ => None,
        }
    }

    /// The decimal text of the integer `term` of type `ty`. A literal's is
    /// exact. Any other is made up, with its sign and how long it may be:
    /// a fact with `str.from_int` leaves Z3 unable to answer the checks
    /// that fail.
    fn decimal(&mut self, term: &str, ty: IntTy) -> Option<String> {
        if let Some(value) = int_literal(term) {
            return smt_string(&value.to_string());
        }
        if let Some(known) = self.text.digits.get(term) {
            return Some(known.clone());
        }
        if is_bound(term) {
            return None;
        }
        let digits = self.unmodeled("the digits of an integer", |exec| {
            exec.fresh_const("String", "digits")
        });
        let longest = ty.lo.to_string().len().max(ty.hi.to_string().len());
        self.describe(
            &digits,
            format!(
                "(and (<= 1 (str.len {digits})) (<= (str.len {digits}) {longest}) (= (str.prefixof \"-\" {digits}) (< {term} 0)) (= (= (str.len {digits}) 1) (and (<= 0 {term}) (<= {term} 9))) (= (= {digits} \"0\") (= {term} 0)))"
            ),
        );
        self.text.digits.insert(term.to_string(), digits.clone());
        Some(digits)
    }

    /// Assumes `term`, which says what the made-up `result` is.
    fn describe(&mut self, result: &str, term: String) {
        self.text
            .described
            .insert(self.facts.len(), result.to_string());
        self.fact(term);
    }

    /// Whether fact `index` says what a made-up result is. Such a fact is
    /// given only to the checks that read the result: a fact about a string
    /// slows every check the solver refutes after it.
    pub(super) fn is_described(&self, index: usize) -> bool {
        self.text.described.contains_key(&index)
    }

    /// For each constant, the facts that say what the made-up results it
    /// follows from are.
    pub(super) fn described_by(&self) -> BTreeMap<&str, BTreeSet<usize>> {
        let mut by: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
        if self.text.described.is_empty() {
            return by;
        }
        let facts: Vec<Vec<&str>> = self
            .facts
            .iter()
            .enumerate()
            .map(|(index, fact)| symbols(&fact[self.guarded.get(&index).copied().unwrap_or(0)..]))
            .collect();
        let mut grew = true;
        while grew {
            grew = false;
            for (index, names) in facts.iter().enumerate() {
                let mut found: BTreeSet<usize> = names
                    .iter()
                    .filter_map(|name| by.get(name))
                    .flatten()
                    .copied()
                    .collect();
                // A description and a definition say what one constant is.
                // Any other fact ties all its constants together.
                let result = match self.text.described.get(&index) {
                    Some(result) => {
                        found.insert(index);
                        symbols(result).first().copied()
                    }
                    None => names
                        .first()
                        .filter(|name| self.definitions.get(**name) == Some(&index))
                        .copied(),
                };
                if found.is_empty() {
                    continue;
                }
                let targets = match result {
                    Some(result) => vec![result],
                    None => names.clone(),
                };
                for target in targets {
                    let known = by.entry(target).or_default();
                    let before = known.len();
                    known.extend(found.iter().copied());
                    grew |= known.len() > before;
                }
            }
        }
        by
    }

    /// The descriptions the check `obligation` reads, as assertions.
    pub(super) fn descriptions(
        &self,
        by: &BTreeMap<&str, BTreeSet<usize>>,
        obligation: &Obligation,
        goal: &str,
    ) -> String {
        if by.is_empty() {
            return String::new();
        }
        let read: BTreeSet<usize> = symbols(goal)
            .into_iter()
            .chain(symbols(&obligation.pc))
            .filter_map(|name| by.get(name))
            .flatten()
            .copied()
            .filter(|index| *index < obligation.facts)
            .collect();
        read.iter()
            .map(|index| format!("(assert {})\n", self.facts[*index]))
            .collect()
    }

    /// Whether `list` is the parts a `str` method returned, unchanged.
    pub(super) fn is_part_list(&self, list: &Val) -> bool {
        matches!(
            list,
            Val::List { data: Some(Store::Leaf(array, _)), .. }
                if self.text.parts.contains_key(array)
        )
    }

    /// A list of the literal `parts`.
    fn literal_list(&mut self, parts: &[String]) -> Option<Val> {
        if parts.len() > MAX_PARTS {
            return None;
        }
        let terms = parts
            .iter()
            .map(|part| smt_string(part))
            .collect::<Option<Vec<_>>>()?;
        // No read reaches past the parts, so the rest of the store is free.
        let mut array = self.fresh_const("(Array Int String)", "parts");
        for (index, term) in terms.iter().enumerate() {
            array = format!("(store {array} {index} {term})");
        }
        let array = self.define("(Array Int String)", "parts", array);
        Some(Val::List {
            len: parts.len().to_string(),
            data: Some(Store::Leaf(array, "String")),
            elem: str_type(),
        })
    }

    /// `value | None`: `value` where `present` holds, else `None`.
    fn optional(&mut self, present: &str, value: Val, member: Type) -> Option<Val> {
        let Type::Union(union) = optional_type(member) else {
            return None;
        };
        let absent = union.members.iter().position(|ty| *ty == Type::Unit)?;
        let tag = format!("(ite {present} {} {absent})", 1 - absent);
        let members = union
            .members
            .iter()
            .map(|ty| match ty {
                Type::Unit => Val::Unit,
                _ => value.clone(),
            })
            .collect();
        Some(Val::Union(tag, members))
    }

    /// A made-up list of parts of `text`, with `count` parts.
    fn part_list(&mut self, what: &str, parts: Parts) -> (String, String, Val) {
        let (count, array) = self.unmodeled(what, |exec| {
            (
                exec.fresh_const("Int", "parts"),
                exec.fresh_const("(Array Int String)", "parts"),
            )
        });
        self.text.parts.insert(array.clone(), parts);
        self.describe(&count, format!("(<= {count} {})", i64::MAX));
        let list = Val::List {
            len: count.clone(),
            data: Some(Store::Leaf(array.clone(), "String")),
            elem: str_type(),
        };
        (count, array, list)
    }

    /// Assumes what holds of the element `value` read at `index` from the
    /// store `data`, when a `str` method returned the list.
    pub(super) fn text_part_facts(&mut self, data: &Store, index: &str, value: &Val) {
        let (Store::Leaf(array, _), Val::Str(part)) = (data, value) else {
            return;
        };
        let Some(parts) = self.text.parts.get(array).cloned() else {
            return;
        };
        if !self.text.read.insert((array.clone(), index.to_string())) {
            return;
        }
        match parts {
            Parts::Chars(text) => {
                self.describe(part, format!("(= {part} (str.at {text} {index}))"));
            }
            Parts::Inside {
                text,
                without,
                filled,
            } => {
                let mut facts = vec![
                    format!("(str.contains {text} {part})"),
                    format!("(<= (str.len {part}) (str.len {text}))"),
                ];
                for other in without {
                    facts.push(format!(
                        "(or (= {other} \"\") (not (str.contains {part} {other})))"
                    ));
                }
                if filled {
                    facts.push(format!("(<= 1 (str.len {part}))"));
                }
                self.describe(part, format!("(and {})", facts.join(" ")));
            }
        }
    }

    /// The result of the `str` method `op` on `text`, and what the runtime
    /// checks before it. `None` when the encoding does not model the call.
    pub(super) fn text_model(
        &mut self,
        op: StrOp,
        text: &str,
        args: &[Val],
    ) -> Option<(Val, Vec<Need>)> {
        let key = format!("{op:?} {text} {args:?}");
        if let Some(known) = self.text.models.get(&key) {
            return Some(known.clone());
        }
        let mut needs = Vec::new();
        let value = self.text_value(op, text, args, &mut needs)?;
        if !is_bound(&key) {
            self.text.models.insert(key, (value.clone(), needs.clone()));
        }
        Some((value, needs))
    }

    fn text_value(
        &mut self,
        op: StrOp,
        text: &str,
        args: &[Val],
        needs: &mut Vec<Need>,
    ) -> Option<Val> {
        let words: Vec<&str> = args
            .iter()
            .map_while(|arg| match arg {
                Val::Str(term) => Some(term.as_str()),
                _ => None,
            })
            .collect();
        let number = match args.first() {
            Some(Val::Int(term, _)) => Some(term.as_str()),
            _ => None,
        };
        let literals = words
            .iter()
            .map(|word| literal(word))
            .collect::<Option<Vec<_>>>();
        if let (Some(text), Some(literals)) = (literal(text), literals) {
            match fold(op, &text, &literals, number.and_then(int_literal)) {
                Some(Folded::Text(value)) if smt_string(&value).is_some() => {
                    return smt_string(&value).map(Val::Str);
                }
                Some(Folded::Count(value)) => {
                    return Some(Val::Int(value.to_string(), IntTy::INT64));
                }
                Some(Folded::List(parts)) => {
                    if let Some(list) = self.literal_list(&parts) {
                        return Some(list);
                    }
                }
                _ => {}
            }
        }
        let len = len_of(text);
        let what = format!("`{}`", RuntimeOp::Str(op).name().trim_start_matches("__"));
        // A made-up result needs a constant, which no bound variable may
        // define.
        let free = !is_bound(text) && !args.iter().any(|arg| is_bound(&format!("{arg:?}")));
        let made = |exec: &mut Self, sort: &str, hint: &str| {
            exec.unmodeled(what.as_str(), |exec| exec.fresh_const(sort, hint))
        };
        if let (
            StrOp::Join,
            [Val::List {
                len: count,
                data: Some(Store::Leaf(array, _)),
                ..
            }],
            true,
        ) = (op, args, free)
        {
            return Some(Val::Str(self.text_join(text, count, array)));
        }
        match (op, words.as_slice(), number) {
            (StrOp::Len, [], None) => Some(Val::Int(len, IntTy::INT64)),
            (StrOp::Contains, [other], _) => {
                Some(Val::Bool(format!("(str.contains {text} {other})")))
            }
            (StrOp::StartsWith, [other], _) => {
                Some(Val::Bool(format!("(str.prefixof {other} {text})")))
            }
            (StrOp::EndsWith, [other], _) => {
                Some(Val::Bool(format!("(str.suffixof {other} {text})")))
            }
            (StrOp::CharAt, [], Some(index)) => {
                needs.push((
                    format!("(and (<= (- {len}) {index}) (< {index} {len}))"),
                    "str index in range",
                ));
                let at = format!("(str.at {text} {})", from_end(&len, index));
                Some(Val::Str(at))
            }
            (StrOp::Slice, [], Some(start)) => {
                // The operands are the start, whether it is written, the
                // end, and whether it is written.
                let written = |index: usize| args.get(index) == Some(&Val::Bool("true".into()));
                let start = match written(1) {
                    true => from_end(&len, start),
                    false => "0".to_string(),
                };
                let end = match (written(3), args.get(2)) {
                    (true, Some(Val::Int(end, _))) => from_end(&len, end),
                    _ => len.clone(),
                };
                needs.push((
                    format!("(and (<= 0 {start}) (<= {start} {end}) (<= {end} {len}))"),
                    "str slice bounds in range",
                ));
                let slice = format!("(str.substr {text} {start} (- {end} {start}))");
                Some(Val::Str(slice))
            }
            (StrOp::Find, [other], _) => {
                let found = format!("(str.indexof {text} {other} 0)");
                let present = format!("(str.contains {text} {other})");
                self.optional(
                    &present,
                    Val::Int(found, IntTy::INT64),
                    Type::named("int64"),
                )
            }
            (StrOp::StripPrefix | StrOp::StripSuffix, [other], _) => {
                let rest = format!("(- {len} (str.len {other}))");
                let (present, from) = match op {
                    StrOp::StripPrefix => ("str.prefixof", len_of(other)),
                    _ => ("str.suffixof", "0".to_string()),
                };
                let kept = format!("(str.substr {text} {from} {rest})");
                self.optional(
                    &format!("({present} {other} {text})"),
                    Val::Str(kept),
                    str_type(),
                )
            }
            (StrOp::SplitOnce, [other], _) => {
                let at = format!("(str.indexof {text} {other} 0)");
                let before = format!("(str.substr {text} 0 {at})");
                let after = format!("(str.substr {text} (+ {at} (str.len {other})) {len})");
                if free {
                    // What the definitions imply, which the solver does
                    // not find by itself. An empty separator stops the
                    // program.
                    self.fact(format!(
                        "(=> (and (str.contains {text} {other}) (not (= {other} \"\"))) (and (= {text} (str.++ {before} {other} {after})) (not (str.contains {before} {other}))))"
                    ));
                }
                let pair = Val::Tuple(vec![Val::Str(before), Val::Str(after)]);
                self.optional(
                    &format!("(str.contains {text} {other})"),
                    pair,
                    Type::Tuple(vec![str_type(), str_type()]),
                )
            }
            (StrOp::Repeat, [], Some(count))
                if matches!(int_literal(count), Some(0..=MAX_COPIES)) =>
            {
                let copies = vec![text; count.parse().unwrap_or(0)];
                Some(Val::Str(match copies.as_slice() {
                    [] => "\"\"".to_string(),
                    [text] => text.to_string(),
                    copies => {
                        let term = format!("(str.++ {})", copies.join(" "));
                        term
                    }
                }))
            }
            _ if !free => None,
            (StrOp::Repeat, [], Some(count)) => {
                let result = made(self, "String", "repeated");
                // A negative count stops the program.
                self.describe(
                    &result,
                    format!(
                        "(=> (>= {count} 0) (and (=> (= {count} 0) (= {result} \"\")) (=> (= {count} 1) (= {result} {text})) (=> (>= {count} 1) (and (str.prefixof {text} {result}) (>= (str.len {result}) {len})))))"
                    ),
                );
                Some(Val::Str(result))
            }
            (StrOp::Replace, [from, to], _) => {
                // The solvers decide little about `str.replace_all`, so
                // the result has the facts that hold of every replacement.
                let result = made(self, "String", "replaced");
                let held = format!("(str.contains {text} {from})");
                let mut facts = vec![
                    format!("(=> (not {held}) (= {result} {text}))"),
                    format!("(=> (= {from} {to}) (= {result} {text}))"),
                    format!("(=> {held} (str.contains {result} {to}))"),
                ];
                if let (Some(from_text), Some(to_text)) = (literal(from), literal(to)) {
                    let relation = match to_text.chars().count().cmp(&from_text.chars().count()) {
                        std::cmp::Ordering::Less => "<=",
                        std::cmp::Ordering::Equal => "=",
                        std::cmp::Ordering::Greater => ">=",
                    };
                    facts.push(format!("({relation} (str.len {result}) {len})"));
                    // A replacement that shares no character with what it
                    // replaces cannot complete a new occurrence of it.
                    let apart = !to_text.chars().any(|ch| from_text.contains(ch));
                    if apart && !to_text.is_empty() && !from_text.is_empty() {
                        facts.push(format!("(not (str.contains {result} {from}))"));
                    }
                }
                self.describe(&result, format!("(and {})", facts.join(" ")));
                Some(Val::Str(result))
            }
            (StrOp::Split, [other], _) => {
                let parts = Parts::Inside {
                    text: text.to_string(),
                    without: vec![other.to_string()],
                    filled: false,
                };
                let (count, array, list) = self.part_list(&what, parts);
                let held = format!("(str.contains {text} {other})");
                // Splitting at nothing gives each character, with an
                // empty part before the first and after the last.
                self.describe(
                    &count,
                    format!(
                    "(and (<= 1 {count}) (ite (= {other} \"\") (= {count} (+ {len} 2)) (<= {count} (+ {len} 1))) (= (>= {count} 2) {held}))"
                ),
                );
                self.describe(
                    &array,
                    format!(
                        "(= (select {array} 0) (ite {held} (str.substr {text} 0 (str.indexof {text} {other} 0)) {text}))"
                    ),
                );
                Some(list)
            }
            (StrOp::Lines, [], None) => {
                let parts = Parts::Inside {
                    text: text.to_string(),
                    without: vec!["\"\\u{a}\"".to_string()],
                    filled: false,
                };
                let (count, _, list) = self.part_list(&what, parts);
                self.describe(
                    &count,
                    format!(
                        "(and (<= 0 {count}) (<= {count} {len}) (= (= {count} 0) (= {text} \"\")))"
                    ),
                );
                Some(list)
            }
            (StrOp::SplitWhitespace, [], None) => {
                let parts = Parts::Inside {
                    text: text.to_string(),
                    without: ["\" \"", "\"\\u{9}\"", "\"\\u{a}\"", "\"\\u{d}\""]
                        .map(str::to_string)
                        .to_vec(),
                    filled: true,
                };
                let (count, _, list) = self.part_list(&what, parts);
                // Each part has a character, and one separates two parts.
                self.describe(
                    &count,
                    format!("(and (<= 0 {count}) (<= (- (* 2 {count}) 1) {len}))"),
                );
                Some(list)
            }
            (StrOp::Chars, [], None) => {
                let (count, _, list) = self.part_list(&what, Parts::Chars(text.to_string()));
                self.describe(&count, format!("(= {count} {len})"));
                Some(list)
            }
            (StrOp::Trim | StrOp::TrimStart | StrOp::TrimEnd, [], None) => {
                let result = made(self, "String", "trimmed");
                let mut facts = vec![
                    format!("(str.contains {text} {result})"),
                    format!("(<= (str.len {result}) {len})"),
                ];
                for space in ["\" \"", "\"\\u{9}\"", "\"\\u{a}\"", "\"\\u{d}\""] {
                    if op != StrOp::TrimEnd {
                        facts.push(format!("(not (str.prefixof {space} {result}))"));
                    }
                    if op != StrOp::TrimStart {
                        facts.push(format!("(not (str.suffixof {space} {result}))"));
                    }
                }
                match op {
                    StrOp::TrimStart => facts.push(format!("(str.suffixof {result} {text})")),
                    StrOp::TrimEnd => facts.push(format!("(str.prefixof {result} {text})")),
                    _ => {}
                }
                self.describe(&result, format!("(and {})", facts.join(" ")));
                Some(Val::Str(result))
            }
            (StrOp::ToLower | StrOp::ToUpper, [], None) => {
                // A character maps to one, two, or three characters.
                let result = made(self, "String", "cased");
                self.describe(
                    &result,
                    format!(
                        "(and (<= {len} (str.len {result})) (<= (str.len {result}) (* 3 {len})))"
                    ),
                );
                Some(Val::Str(result))
            }
            (StrOp::ByteLen, [], None) => {
                // UTF-8 writes a character in one to four bytes.
                let result = made(self, "Int", "bytes");
                self.describe(
                    &result,
                    format!(
                        "(and (<= {len} {result}) (<= {result} (* 4 {len})) (<= {result} {}))",
                        i64::MAX
                    ),
                );
                Some(Val::Int(result, IntTy::INT64))
            }
            (StrOp::Count, [other], _) => {
                let result = made(self, "Int", "count");
                // Matches do not overlap. An empty text to count stops
                // the program.
                self.describe(
                    &result,
                    format!(
                    "(or (= {other} \"\") (and (<= 0 {result}) (<= {result} {}) (<= (* {result} {}) {len}) (= (>= {result} 1) (str.contains {text} {other}))))",
                    i64::MAX,
                    literal(other).map_or(1, |other| other.chars().count())
                ),
                );
                Some(Val::Int(result, IntTy::INT64))
            }
            _ => None,
        }
    }

    /// `text.join(parts)` for the `count` parts in the store `array`.
    fn text_join(&mut self, text: &str, count: &str, array: &str) -> String {
        let joined = |parts: i128| match parts {
            0 => "\"\"".to_string(),
            1 => format!("(select {array} 0)"),
            _ => {
                let mut term = format!("(str.++ (select {array} 0)");
                for index in 1..parts {
                    term.push_str(&format!(" {text} (select {array} {index})"));
                }
                term + ")"
            }
        };
        if let Some(parts @ 0..=MAX_COPIES) = int_literal(count) {
            return joined(parts);
        }
        let result = self.unmodeled("`join`", |exec| exec.fresh_const("String", "joined"));
        for parts in 0..=3 {
            self.describe(
                &result,
                format!("(=> (= {count} {parts}) (= {result} {}))", joined(parts)),
            );
        }
        // The separators alone are that long.
        let separator = literal(text).map_or(0, |text| text.chars().count());
        self.describe(
            &result,
            format!("(>= (str.len {result}) (* (- {count} 1) {separator}))"),
        );
        result
    }

    /// A `str` method the function body calls: its result, and a check of
    /// what the runtime checks.
    pub(super) fn text_call(
        &mut self,
        state: &State,
        span: Span,
        op: StrOp,
        text: &str,
        args: &[Val],
    ) -> Option<Val> {
        let (value, needs) = self.text_model(op, text, args)?;
        for (goal, message) in needs {
            self.oblige(
                ObligationKind::IndexInRange,
                span,
                message,
                &state.pc,
                Goal::Holds(goal),
            );
        }
        Some(value)
    }

    /// `text.to_bytes()`: the UTF-8 bytes of a `str`. Their values are
    /// made up; their count is exact for a literal, and else between one
    /// and four for each character.
    pub(super) fn text_bytes(&mut self, text: &Val) -> Option<Val> {
        let Val::Str(text) = text else { return None };
        let elem = Type::named("uint8");
        let what = "`str.to_bytes`";
        let data = self.unmodeled(what, |exec| exec.fresh_store(&elem, "bytes"));
        let len = match literal(text) {
            Some(text) => text.len().to_string(),
            None => {
                let count = self.unmodeled(what, |exec| exec.fresh_const("Int", "bytes"));
                let len = len_of(text);
                self.describe(
                    &count,
                    format!(
                        "(and (<= {len} {count}) (<= {count} (* 4 {len})) (<= {count} {}))",
                        i64::MAX
                    ),
                );
                count
            }
        };
        Some(Val::List { len, data, elem })
    }

    /// `to_string()` on an integer or a `bool`.
    pub(super) fn text_of(&mut self, value: Option<&Val>) -> Option<Val> {
        self.display(value?).map(Val::Str)
    }

    /// An f-string: its parts one after another. A part the encoding has
    /// no text for is a made-up string.
    pub(super) fn format_string(&mut self, state: &mut State, parts: &[MirFormatPart]) -> Val {
        let mut terms = Vec::new();
        for part in parts {
            let known = match part {
                MirFormatPart::Literal(text) => smt_string(text),
                MirFormatPart::Value(operand) | MirFormatPart::Int64(operand) => {
                    let value = self.operand(state, operand, None);
                    let scalar = match (&value, self.operand_type(operand)) {
                        (Val::Int(..), Some(ty)) => IntTy::of(&ty).is_some(),
                        (Val::Str(_), Some(ty)) => is_str(&ty),
                        (Val::Bool(_), _) => true,
                        _ => false,
                    };
                    scalar.then(|| self.display(&value)).flatten()
                }
                MirFormatPart::Formatted { value, .. } => {
                    self.operand(state, value, None);
                    None
                }
            };
            terms.push(match known {
                Some(term) => term,
                None => self.unmodeled("an f-string", |exec| exec.fresh_const("String", "part")),
            });
        }
        let term = match terms.as_slice() {
            [] => "\"\"".to_string(),
            [term] => term.clone(),
            terms => format!("(str.++ {})", terms.join(" ")),
        };
        // A short term is used in place: most f-strings are the message of
        // a check, which no other check reads.
        Val::Str(if term.len() > MAX_INLINE {
            self.define("String", "text", term)
        } else {
            term
        })
    }

    /// A `str` method a contract calls, by name. Each condition the call
    /// needs to be well-defined is pushed to `wd`.
    pub(super) fn spec_text(
        &mut self,
        text: &str,
        method: &str,
        args: &[Val],
        wd: &mut Vec<String>,
    ) -> Result<Val, String> {
        let unknown = || format!("`str.{method}` is not modeled in contracts yet");
        if method == "clone" {
            return Ok(Val::Str(text.to_string()));
        }
        let op = StrOp::ALL
            .iter()
            .find(|op| RuntimeOp::Str(**op).name() == method)
            .ok_or_else(unknown)?;
        let (value, needs) = self.text_model(*op, text, args).ok_or_else(unknown)?;
        wd.extend(needs.into_iter().map(|(goal, _)| goal));
        Ok(value)
    }

    /// `text[start:end]` in a contract.
    pub(super) fn spec_slice(
        &mut self,
        object: &Expr,
        bounds: [&Option<Box<Expr>>; 2],
        env: &BTreeMap<String, Val>,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<Val, String> {
        let Val::Str(text) = self.spec(object, env, wd, depth)? else {
            return Err("a slice of this type is not modeled in contracts yet".into());
        };
        let mut args = Vec::new();
        for bound in bounds {
            args.push(match bound {
                Some(bound) => self.spec(bound, env, wd, depth)?,
                None => Val::Int("0".to_string(), IntTy::INT64),
            });
            args.push(Val::Bool(bound.is_some().to_string()));
        }
        self.spec_text(&text, "__slice", &args, wd)
    }

    /// An f-string in a contract.
    pub(super) fn spec_format(
        &mut self,
        parts: &[FormatPart],
        env: &BTreeMap<String, Val>,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<Val, String> {
        let mut terms = Vec::new();
        for part in parts {
            let term = match part {
                FormatPart::Literal(text) => smt_string(text),
                FormatPart::Expr(expr) => {
                    let value = self.spec(expr, env, wd, depth)?;
                    self.display(&value)
                }
                FormatPart::Formatted { .. } => None,
            };
            terms.push(term.ok_or("this f-string part is not modeled in contracts yet")?);
        }
        Ok(Val::Str(match terms.as_slice() {
            [] => "\"\"".to_string(),
            [term] => term.clone(),
            terms => format!("(str.++ {})", terms.join(" ")),
        }))
    }

    /// `value is None` in a contract, for a `T | None` such as `find`
    /// returns.
    pub(super) fn spec_is_none(&self, value: &Val, negated: bool) -> Result<Val, String> {
        match value {
            Val::Union(tag, members) => {
                let none = members.iter().position(|member| *member == Val::Unit);
                let none = none.ok_or("`is None` on a value that is never `None`")?;
                let relation = if negated { "distinct" } else { "=" };
                Ok(Val::Bool(format!("({relation} {tag} {none})")))
            }
            _ => Err("`is None` on this value is not modeled in contracts yet".into()),
        }
    }

    /// `str(value)` in a contract.
    pub(super) fn spec_display(&mut self, value: &Val) -> Result<Val, String> {
        self.display(value)
            .map(Val::Str)
            .ok_or("the text of this value is not modeled in contracts yet".to_string())
    }
}
