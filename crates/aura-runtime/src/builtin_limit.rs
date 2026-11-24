//! The evaluator sets an output allowance around a single deterministic builtin.
//! Normal Aura calls have the builtin's usual output limit.
use crate::diag::{Diagnostic, Result};
use std::cell::Cell;

thread_local! {
    static ALLOWANCE: Cell<Option<usize>> = const { Cell::new(None) };
}

pub fn with_allowance<T>(bytes: usize, f: impl FnOnce() -> T) -> T {
    ALLOWANCE.with(|cell| {
        struct Restore<'a>(&'a Cell<Option<usize>>, Option<usize>);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.set(self.1);
            }
        }
        let _restore = Restore(cell, cell.replace(Some(bytes)));
        f()
    })
}

pub(crate) fn allowance() -> Option<usize> {
    ALLOWANCE.get()
}

pub(crate) fn check(bytes: u64) -> Result<()> {
    if allowance().is_some_and(|available| bytes > available as u64) {
        return Err(Diagnostic::coded(
            "AU3028",
            "constant evaluation exceeds the memory limit of 8 MiB",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_output_bound_restores_after_unwind() {
        assert!(check(u64::MAX).is_ok());
        with_allowance(5, || {
            assert!(check(5).is_ok());
            let trapped = std::panic::catch_unwind(|| {
                with_allowance(7, || {
                    assert!(check(7).is_ok());
                    panic!("unwind inner allowance");
                })
            });
            assert!(trapped.is_err());
            assert_eq!(check(6).unwrap_err().code, "AU3028");
        });
        assert!(check(u64::MAX).is_ok());
    }
}
