//! Transient pattern indices are local to one initializer. Searching with
//! an index recompiles from its text within the current output allowance.
use super::*;
use crate::diag::Diagnostic;

#[derive(Default)]
struct Table {
    texts: Vec<Arc<str>>,
    bytes: usize,
}

thread_local! {
    static LOCAL: RefCell<Table> = RefCell::new(Table::default());
}

pub fn with_local_patterns<T>(operation: impl FnOnce() -> T) -> T {
    struct Restore(Table);
    impl Drop for Restore {
        fn drop(&mut self) {
            LOCAL.with(|table| table.replace(std::mem::take(&mut self.0)));
        }
    }
    let _restore = Restore(LOCAL.with(RefCell::take));
    operation()
}

pub(super) fn number(pattern: &str) -> std::result::Result<i64, String> {
    LOCAL.with(|cell| {
        let mut table = cell.borrow_mut();
        if let Some(index) = table.texts.iter().position(|s| s.as_ref() == pattern) {
            return Ok(index as i64);
        }
        let bytes = table.bytes.saturating_add(pattern.len()).saturating_add(16);
        crate::builtin_limit::check(bytes as u64).map_err(|error| error.message)?;
        compile_local(pattern, bytes)?;
        let index = table.texts.len();
        table.texts.push(Arc::from(pattern));
        table.bytes = bytes;
        Ok(index as i64)
    })
}

pub(super) fn text(number: i64) -> Result<Arc<str>> {
    LOCAL.with(|table| {
        table
            .borrow()
            .texts
            .get(usize::try_from(number).unwrap_or(usize::MAX))
            .cloned()
            .ok_or_else(|| Diagnostic::new("invalid local regex pattern"))
    })
}

fn compile_local(pattern: &str, table_bytes: usize) -> std::result::Result<Rc<Regex>, String> {
    let available = crate::builtin_limit::allowance()
        .unwrap_or(0)
        .saturating_sub(table_bytes);
    regex::RegexBuilder::new(pattern)
        .size_limit(available.saturating_mul(3) / 4)
        .dfa_size_limit(available / 4)
        .build()
        .map(Rc::new)
        .map_err(|error| {
            if matches!(error, regex::Error::CompiledTooBig(_)) {
                crate::builtin_limit::fits(u64::MAX);
            }
            error.to_string()
        })
}

pub(super) fn compiled(number: i64) -> Result<Rc<Regex>> {
    let pattern = text(number)?;
    LOCAL
        .with(|table| compile_local(&pattern, table.borrow().bytes))
        .map_err(Diagnostic::new)
}

pub(super) fn bound_search(
    name: &str,
    regex: &Regex,
    text: &str,
    arg: &RegexArgument<'_>,
) -> Result<()> {
    let length = text.len() as u64;
    let bytes = match name {
        "regex::__aura_find" => regex.find(text).map_or(0, |m| m.len() as u64 + 32),
        "regex::__aura_find_all" | "regex::__aura_matches" | "regex::__aura_split" => {
            let count = regex.find_iter(text).count() as u64;
            length.saturating_add((count + 1).saturating_mul(32))
        }
        "regex::__aura_captures" => regex.captures(text).map_or(0, |caps| {
            caps.iter().fold(0u64, |sum, cap| {
                sum.saturating_add(16)
                    .saturating_add(cap.map_or(0, |m| m.len()) as u64)
            })
        }),
        "regex::__aura_replace" | "regex::__aura_replace_all" => {
            let RegexArgument::Text(replacement) = arg else {
                return Ok(());
            };
            let references = replacement.bytes().filter(|b| *b == b'$').count() as u64;
            let count = if name.ends_with("_all") {
                regex.find_iter(text).count() as u64
            } else {
                1
            };
            length
                .saturating_mul(1 + references)
                .saturating_add(count.saturating_mul(replacement.len() as u64))
        }
        _ => 0,
    };
    crate::builtin_limit::check(bytes)
}
