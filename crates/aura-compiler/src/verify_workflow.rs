//! The workflow around a verified module: which outcomes fail a run, the
//! functions that must verify completely, and a run over several files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{Options, Report, Status};
use crate::diag::{Diagnostic, Span};
use crate::mir::MirModule;
use crate::sema::Program;

/// The open outcomes, besides a failed check, that fail a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deny {
    pub not_modeled: bool,
    pub undecided: bool,
}

impl Deny {
    /// Every open outcome fails the run.
    pub const ALL: Deny = Deny {
        not_modeled: true,
        undecided: true,
    };

    /// Adds the class a `--deny` argument names: `not-modeled`,
    /// `undecided`, or `all`. Returns whether `class` is one of them.
    pub fn add(&mut self, class: &str) -> bool {
        match class {
            "not-modeled" => self.not_modeled = true,
            "undecided" => self.undecided = true,
            "all" => *self = Self::ALL,
            _ => return false,
        }
        true
    }
}

/// A function that must verify completely, and how many of its checks are
/// not proved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MustVerify {
    pub function: String,
    pub span: Span,
    pub open: usize,
}

impl MustVerify {
    pub(super) fn diagnostic(&self) -> Diagnostic {
        Diagnostic::coded_at(
            "AU5122",
            self.span,
            format!(
                "function `{}` marked verified has unproved checks",
                self.function
            ),
        )
        .with_note(format!(
            "checks not proved: {}; each has its own diagnostic",
            self.open
        ))
    }
}

impl Report {
    /// The functions that must verify and have a check that is not proved.
    pub fn unverified(&self) -> impl Iterator<Item = &MustVerify> {
        self.must_verify.iter().filter(|marked| marked.open > 0)
    }

    /// Whether the run passes: no check failed, every function that must
    /// verify did, and no check has an outcome `deny` names.
    pub fn passes(&self, deny: Deny) -> bool {
        self.failed() == 0
            && self.unverified().next().is_none()
            && !(deny.not_modeled && self.not_modeled() > 0)
            && !(deny.undecided && self.undecided() > 0)
    }
}

/// The functions `names` asks to verify completely that the report covers,
/// each with its count of open checks.
pub(super) fn must_verify(
    names: &[String],
    spans: &BTreeMap<String, Span>,
    report: &Report,
) -> Vec<MustVerify> {
    names
        .iter()
        .filter_map(|name| {
            let span = *spans.get(name)?;
            let open = report
                .obligations
                .iter()
                .filter(|obligation| {
                    obligation.function == *name && obligation.status != Status::Proved
                })
                .count();
            Some(MustVerify {
                function: name.clone(),
                span,
                open,
            })
        })
        .collect()
}

/// The names in `names` that no report in `reports` covers. A name that
/// matches nothing is most likely misspelled, and would otherwise pass.
pub fn unmatched_must_verify<'a>(
    names: &'a [String],
    reports: impl IntoIterator<Item = &'a Report>,
) -> Vec<&'a str> {
    let matched: BTreeSet<&str> = reports
        .into_iter()
        .flat_map(|report| &report.must_verify)
        .map(|marked| marked.function.as_str())
        .collect();
    names
        .iter()
        .map(String::as_str)
        .filter(|name| !matched.contains(name))
        .collect()
}

/// The name a report shows for MIR function `name`, and whether the
/// function is a copy the checker made for a generator argument. Such a
/// copy is reported as the function it copies.
pub(super) fn reported_name(name: &str) -> (String, bool) {
    let public = crate::mir::public_function_name(name);
    let original = public
        .find(crate::diag::GENERATOR_COPY_PREFIX)
        .and_then(|at| {
            let original = crate::diag::generator_copy_original(&public[at..])?;
            Some(format!("{}{original}", &public[..at]))
        });
    match original {
        Some(original) => (original, true),
        None if public == MODULE_BODY_FUNCTION => (MODULE_BODY.to_string(), false),
        None => match public.strip_prefix(CONSTANT_INITIALIZER_PREFIX) {
            // `<module>::<constant>`: the constant's name says enough.
            Some(constant) => {
                let name = constant.rsplit("::").next().unwrap_or(constant);
                (name.to_string(), false)
            }
            None => (public, false),
        },
    }
}

/// Prefix of the MIR function that computes a module constant's value.
const CONSTANT_INITIALIZER_PREFIX: &str = "__aura_const_init::";

/// The MIR function that holds a module's top-level statements.
const MODULE_BODY_FUNCTION: &str = "__script";

/// The name a report shows for the module's top-level statements.
pub const MODULE_BODY: &str = "<module>";

/// A failed check's status as a copy made for a generator argument should
/// report it: without the values of the names the checker made up.
pub(super) fn copy_status(status: Status) -> Status {
    match status {
        Status::Failed { counterexample } => Status::Failed {
            counterexample: counterexample
                .into_iter()
                .filter(|(name, _)| !name.starts_with("__aura_"))
                .collect(),
        },
        other => other,
    }
}

/// The modules other than the entry module and the standard library whose
/// functions the program holds. Their bodies are verified only when the
/// module is itself the entry of a run.
pub(super) fn unchecked_modules(program: &Program, module: &MirModule) -> Vec<String> {
    let modules: BTreeSet<&str> = module
        .functions
        .iter()
        .map(|function| function.module_name.as_str())
        .filter(|name| *name != program.module_name)
        .filter(|name| {
            let root = name.split('.').next().unwrap_or(name);
            !crate::builtin_modules::is_builtin_root_module(root)
        })
        .collect();
    modules.into_iter().map(str::to_string).collect()
}

/// The directory that holds the modules of the package at `dir`: its `src`
/// directory when there is one, else `dir` itself.
pub fn package_source_root(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    if src.is_dir() {
        src
    } else {
        dir.to_path_buf()
    }
}

/// What verifying one file of a package gave: its report, or the
/// diagnostic that stopped it.
#[derive(Clone, Debug)]
pub struct FileReport {
    pub path: PathBuf,
    pub source: String,
    pub result: std::result::Result<Report, Diagnostic>,
}

/// The reports of the files of one run, in the order given.
#[derive(Clone, Debug, Default)]
pub struct PackageReport {
    pub files: Vec<FileReport>,
}

/// The counts of a run over several files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub files: usize,
    /// Files that did not compile, or for which no solver ran.
    pub errors: usize,
    pub functions: usize,
    pub cached: usize,
    pub checks: usize,
    pub proved: usize,
    pub failed: usize,
    pub not_modeled: usize,
    pub undecided: usize,
}

impl PackageReport {
    /// The reports of the files that were verified.
    pub fn reports(&self) -> impl Iterator<Item = &Report> {
        self.files
            .iter()
            .filter_map(|file| file.result.as_ref().ok())
    }

    pub fn totals(&self) -> Totals {
        let mut totals = Totals {
            files: self.files.len(),
            errors: self.files.len() - self.reports().count(),
            ..Totals::default()
        };
        for report in self.reports() {
            totals.functions += report.functions;
            totals.cached += report.cached;
            totals.checks += report.obligations.len();
            totals.proved += report.proved();
            totals.failed += report.failed();
            totals.not_modeled += report.not_modeled();
            totals.undecided += report.undecided();
        }
        totals
    }

    /// Whether every file was verified and passes under `deny`.
    pub fn passes(&self, deny: Deny) -> bool {
        self.files
            .iter()
            .all(|file| file.result.as_ref().is_ok_and(|report| report.passes(deny)))
    }
}

/// Verifies each file in `paths` as the entry module of its own run, so
/// every module of a package is verified once. A file that does not
/// compile is recorded and the run goes on. A run with no solver stops at
/// the first file, since no later file can do better.
pub fn verify_files(paths: &[PathBuf], options: &Options) -> PackageReport {
    let mut package = PackageReport::default();
    for path in paths {
        let result = crate::verify_path(path, options);
        let stop = matches!(&result, Err(error) if error.code == "AU5103");
        package.files.push(FileReport {
            path: path.clone(),
            // The text only illustrates diagnostics, and a file that cannot
            // be read already has its own.
            source: std::fs::read_to_string(path).unwrap_or_default(),
            result,
        });
        if stop {
            break;
        }
    }
    package
}
