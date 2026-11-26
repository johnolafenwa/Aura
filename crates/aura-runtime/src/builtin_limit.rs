//! A scoped output allowance for deterministic builtins invoked by the compiler.
use crate::diag::{Diagnostic, Result};
use std::cell::Cell;

#[derive(Clone, Copy)]
struct Allowance {
    bytes: usize,
    exceeded: bool,
}

thread_local! {
    static ALLOWANCE: Cell<Option<Allowance>> = const { Cell::new(None) };
}

pub fn with_allowance<T>(bytes: usize, f: impl FnOnce() -> Result<T>) -> Result<T> {
    ALLOWANCE.with(|cell| {
        struct Restore<'a>(&'a Cell<Option<Allowance>>, Option<Allowance>);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.set(self.1);
            }
        }
        let _restore = Restore(
            cell,
            cell.replace(Some(Allowance {
                bytes,
                exceeded: false,
            })),
        );
        let result = f();
        if cell.get().is_some_and(|limit| limit.exceeded) {
            Err(exhausted())
        } else {
            result
        }
    })
}

pub fn active() -> bool {
    ALLOWANCE.get().is_some()
}

pub(crate) fn allowance() -> Option<usize> {
    ALLOWANCE.get().map(|limit| limit.bytes)
}

// Some builtins return a language-level Result or None on failure. Remember
// exhaustion until the enclosing evaluator call returns through that API.
pub(crate) fn fits(bytes: u64) -> bool {
    if let Some(mut limit) = ALLOWANCE.get() {
        if limit.exceeded {
            return false;
        }
        if bytes > limit.bytes as u64 {
            limit.exceeded = true;
            ALLOWANCE.set(Some(limit));
            return false;
        }
    }
    true
}

pub(crate) fn exhausted() -> Diagnostic {
    Diagnostic::coded(
        "AU3028",
        "constant evaluation exceeds the memory limit of 8 MiB",
    )
}

pub(crate) fn check(bytes: u64) -> Result<()> {
    if fits(bytes) {
        Ok(())
    } else {
        Err(exhausted())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_output_bound_restores_after_unwind_and_escapes_codec_errors() {
        assert!(check(u64::MAX).is_ok());
        let result = with_allowance(5, || {
            assert!(check(5).is_ok());
            let trapped = std::panic::catch_unwind(|| {
                with_allowance::<()>(7, || {
                    assert!(check(7).is_ok());
                    panic!("unwind inner allowance");
                })
            });
            assert!(trapped.is_err());
            assert!(!fits(6));
            Ok(false)
        });
        assert_eq!(result.unwrap_err().code, "AU3028");
        assert!(check(u64::MAX).is_ok());
    }
}
