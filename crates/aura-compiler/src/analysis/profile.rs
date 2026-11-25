//! The checking profile for a single editor request, including its imports,
//! completion recovery attempts and any rename rechecks.

use std::path::Path;

use crate::ast::Module;
use crate::diag::{Diagnostic, Result};
use crate::sema::Program;

/// Selects a profile for the duration of an editor request. Calls on other
/// threads and later requests keep their own profile.
pub fn with_freestanding_analysis<T>(enabled: bool, operation: impl FnOnce() -> T) -> T {
    let _config = if enabled {
        crate::enter_freestanding_build_config()
    } else {
        let current = crate::current_build_config();
        current.freestanding.then(|| {
            crate::BuildConfig {
                cfg: current.cfg,
                debug: current.debug,
                release: current.release,
                ..crate::BuildConfig::default()
            }
            .enter()
        })
    };
    operation()
}

pub(super) fn check(path: &Path, source: &str) -> Result<Program> {
    if crate::current_build_config().freestanding {
        crate::check_path_with_source_without_lockfile_profile(path, source, true)
    } else {
        crate::check_path_with_source_without_lockfile(path, source)
    }
}

pub(super) fn parse(source: &str) -> Result<Module> {
    crate::parser::parse_all_syntax_with_module_scope(
        source,
        crate::current_build_config().freestanding,
    )
}

pub(super) fn diagnostics(program: &Program) -> Vec<Diagnostic> {
    if crate::current_build_config().freestanding {
        crate::check_freestanding(&crate::mir::lower_with_source_locations(program))
    } else {
        Vec::new()
    }
}
