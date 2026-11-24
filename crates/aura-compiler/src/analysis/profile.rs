//! The checking profile for a single editor request, including its imports,
//! completion recovery attempts and any rename rechecks.

use std::cell::Cell;
use std::path::Path;

use crate::ast::Module;
use crate::diag::{Diagnostic, Result};
use crate::sema::Program;

thread_local! {
    static FREESTANDING: Cell<bool> = const { Cell::new(false) };
}

/// Selects a profile for the duration of an editor request. Calls on other
/// threads and later requests keep their own profile.
pub fn with_freestanding_analysis<T>(enabled: bool, operation: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            FREESTANDING.set(self.0);
        }
    }
    let _restore = Restore(FREESTANDING.replace(enabled));
    operation()
}

pub(super) fn check(path: &Path, source: &str) -> Result<Program> {
    if FREESTANDING.get() {
        crate::check_path_with_source_without_lockfile_profile(path, source, true)
    } else {
        crate::check_path_with_source_without_lockfile(path, source)
    }
}

pub(super) fn parse(source: &str) -> Result<Module> {
    crate::parser::parse_with_module_scope(source, FREESTANDING.get())
}

pub(super) fn diagnostics(program: &Program) -> Vec<Diagnostic> {
    if FREESTANDING.get() {
        crate::check_freestanding(&crate::mir::lower_with_source_locations(program))
    } else {
        Vec::new()
    }
}
