//! `aura verify`: static proof that a program's checks cannot fail
//! (verification checkpoint stage V2).
//!
//! The verifier lowers the checked program to MIR, names every trap
//! (`mir::checks`), and stops before the ownership pass, since retains and
//! releases carry no logical meaning. Each function is then executed
//! symbolically ([`exec`]) into SMT-LIB obligations, which an external
//! solver ([`smt`]) proves or refutes one at a time. A function is proved
//! modularly: a call relies only on its callee's contract ([`spec`]).
//!
//! Every obligation is reported as proved, failed with a counterexample,
//! unknown (the solver gave up), or unsupported (the step is outside the
//! subset the encoding models). Only proved obligations count as proof.

#[path = "verify_exec.rs"]
mod exec;
#[path = "verify_smt.rs"]
pub mod smt;
#[path = "verify_spec.rs"]
mod spec;

use std::collections::BTreeMap;

use crate::ast::{ContractClause, ContractKind, FunctionDecl};
use crate::diag::{Diagnostic, Span};
use crate::mir::{MirClass, MirEnum, MirFunction, MirModule};
use crate::sema::{Program, Type};

/// What `aura verify` proves about one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObligationKind {
    /// An integer result fits its type.
    Overflow,
    /// A divisor is not zero.
    DivisionByZero,
    /// A list index selects an element.
    IndexInRange,
    /// An `assert` statement holds.
    Assert,
    /// A callee's `requires` clause holds at a call.
    Requires,
    /// An `ensures` clause holds on return.
    Ensures,
    /// A loop `invariant` holds on entry and after every iteration.
    LoopInvariant,
    /// A `decreases` measure stays at least 0 and gets smaller.
    Decreases,
    /// A class invariant holds where it is checked.
    ClassInvariant,
}

impl ObligationKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Overflow => "overflow",
            Self::DivisionByZero => "division by zero",
            Self::IndexInRange => "index in range",
            Self::Assert => "assert",
            Self::Requires => "requires",
            Self::Ensures => "ensures",
            Self::LoopInvariant => "loop invariant",
            Self::Decreases => "decreases",
            Self::ClassInvariant => "class invariant",
        }
    }
}

/// The outcome of one obligation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    Proved,
    /// The solver found inputs that break the check.
    Failed {
        counterexample: Vec<(String, String)>,
    },
    /// The solver gave up or ran out of time.
    Unknown,
    /// The check depends on a step the encoding does not model.
    Unsupported(String),
}

impl Status {
    fn rank(&self) -> u8 {
        match self {
            Status::Proved => 0,
            Status::Unsupported(_) => 1,
            Status::Unknown => 2,
            Status::Failed { .. } => 3,
        }
    }
}

/// One source check and what the verifier showed about it. A check that
/// several paths or copies of a generic function reach is proved only when
/// every one of them is.
#[derive(Clone, Debug)]
pub struct ObligationReport {
    pub kind: ObligationKind,
    pub function: String,
    pub span: Span,
    pub message: String,
    pub status: Status,
}

/// The result of `aura verify`.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub solver: String,
    pub functions: usize,
    /// How many functions the proof cache answered.
    pub cached: usize,
    pub obligations: Vec<ObligationReport>,
}

impl Report {
    pub fn proved(&self) -> usize {
        self.obligations
            .iter()
            .filter(|obligation| obligation.status == Status::Proved)
            .count()
    }

    /// Whether every obligation was proved.
    pub fn is_proved(&self) -> bool {
        self.proved() == self.obligations.len()
    }

    /// A diagnostic for every obligation that was not proved, in source
    /// order.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.obligations
            .iter()
            .filter_map(|obligation| {
                let what = &obligation.message;
                let diagnostic = match &obligation.status {
                    Status::Proved => return None,
                    Status::Failed { counterexample } => {
                        let mut diagnostic = Diagnostic::coded_at(
                            "AU5101",
                            obligation.span,
                            format!("cannot prove {what}"),
                        )
                        .with_note(format!("in `{}` ({} check)", obligation.function, obligation.kind.name()));
                        if !counterexample.is_empty() {
                            let values = counterexample
                                .iter()
                                .map(|(name, value)| format!("{name} = {value}"))
                                .collect::<Vec<_>>()
                                .join(", ");
                            diagnostic =
                                diagnostic.with_note(format!("counterexample: {values}"));
                        }
                        diagnostic
                    }
                    Status::Unknown => Diagnostic::coded_at(
                        "AU5102",
                        obligation.span,
                        format!("the solver could not decide {what}"),
                    )
                    .with_note(format!("in `{}` ({} check)", obligation.function, obligation.kind.name()))
                    .with_help("raise the limit with `--timeout <ms>`, or add a `requires`, `invariant`, or `assert` that narrows the case"),
                    Status::Unsupported(reason) => Diagnostic::coded_at(
                        "AU5104",
                        obligation.span,
                        format!("cannot check {what}: {reason}"),
                    )
                    .with_note(format!("in `{}` ({} check)", obligation.function, obligation.kind.name())),
                };
                Some(diagnostic)
            })
            .collect()
    }
}

/// How to run the verifier.
#[derive(Clone, Debug)]
pub struct Options {
    /// The solver program; `AURA_SMT_SOLVER` or `PATH` when `None`.
    pub solver: Option<String>,
    /// The limit for each query, in milliseconds.
    pub timeout_ms: u64,
    /// Only functions whose name contains this text.
    pub function_filter: Option<String>,
    /// Print each query's SMT text to standard error. The default is
    /// whether `AURA_VERIFY_TRACE` is set.
    pub trace: bool,
    /// The proof cache directory; `None` solves every query.
    pub cache_dir: Option<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            solver: None,
            timeout_ms: 10_000,
            function_filter: None,
            trace: std::env::var_os("AURA_VERIFY_TRACE").is_some(),
            cache_dir: None,
        }
    }
}

/// What the executor reads about the program: MIR functions, classes and
/// enums by name, and the declarations that hold contracts.
pub(crate) struct VerifyContext<'a> {
    functions: BTreeMap<&'a str, &'a MirFunction>,
    classes: BTreeMap<&'a str, &'a MirClass>,
    enums: BTreeMap<&'a str, &'a MirEnum>,
    decls: BTreeMap<String, &'a FunctionDecl>,
    spec_defs: BTreeMap<&'a str, &'a FunctionDecl>,
    class_invariants: BTreeMap<String, Vec<ContractClause>>,
}

impl<'a> VerifyContext<'a> {
    fn new(program: &'a Program, module: &'a MirModule) -> Self {
        let mut decls = BTreeMap::new();
        let mut spec_defs = BTreeMap::new();
        for (name, function) in &program.functions {
            decls.insert(name.clone(), &function.decl);
            if function.decl.spec {
                spec_defs.insert(name.as_str(), &function.decl);
            }
        }
        let mut class_invariants = BTreeMap::new();
        for class in program.classes.values() {
            for (name, method) in &class.methods {
                decls.insert(format!("{}.{name}", class.decl.name), &method.decl);
                if name == crate::ast::CLASS_INVARIANT_METHOD {
                    class_invariants.insert(
                        class.decl.name.clone(),
                        method
                            .decl
                            .contracts
                            .iter()
                            .filter(|clause| clause.kind == ContractKind::Invariant)
                            .cloned()
                            .collect(),
                    );
                }
            }
        }
        Self {
            functions: module
                .functions
                .iter()
                .map(|function| (function.name.as_str(), function))
                .collect(),
            classes: module
                .classes
                .iter()
                .map(|class| (class.name.as_str(), class))
                .collect(),
            enums: module
                .enums
                .iter()
                .map(|item| (item.name.as_str(), item))
                .collect(),
            decls,
            spec_defs,
            class_invariants,
        }
    }

    pub(crate) fn function(&self, name: &str) -> Option<&'a MirFunction> {
        self.functions.get(name).copied()
    }

    /// The declaration whose contract a call of MIR function `name` uses.
    pub(crate) fn decl(&self, name: &str) -> Option<&'a FunctionDecl> {
        self.decls.get(spec::base_name(name)).copied()
    }

    pub(crate) fn spec_def(&self, name: &str) -> Option<&'a FunctionDecl> {
        self.spec_defs.get(name).copied()
    }

    pub(crate) fn class_invariant(&self, class: &str) -> Option<&[ContractClause]> {
        self.class_invariants.get(class).map(Vec::as_slice)
    }

    /// Each field of class `name` and its type for `args`.
    pub(crate) fn class_fields(&self, name: &str, args: &[Type]) -> Option<Vec<(String, Type)>> {
        let class = self.classes.get(name)?;
        if class.type_params.len() != args.len() {
            return None;
        }
        let substitutions: BTreeMap<&str, &Type> = class
            .type_params
            .iter()
            .map(String::as_str)
            .zip(args)
            .collect();
        Some(
            class
                .fields
                .iter()
                .map(|field| (field.name.clone(), substitute(&field.ty, &substitutions)))
                .collect(),
        )
    }

    /// Each variant of enum `name` and its payload types for `args`.
    pub(crate) fn enum_variants(
        &self,
        name: &str,
        args: &[Type],
    ) -> Option<Vec<(String, Vec<Type>)>> {
        let item = self.enums.get(name)?;
        if item.type_params.len() != args.len() {
            return None;
        }
        let substitutions: BTreeMap<&str, &Type> = item
            .type_params
            .iter()
            .map(String::as_str)
            .zip(args)
            .collect();
        Some(
            item.variants
                .iter()
                .map(|variant| {
                    (
                        variant.name.clone(),
                        variant
                            .payloads
                            .iter()
                            .map(|ty| substitute(ty, &substitutions))
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    pub(crate) fn variant_index(&self, name: &str, variant: &str) -> Option<usize> {
        self.enums
            .get(name)?
            .variants
            .iter()
            .position(|candidate| candidate.name == variant)
    }

    /// The type of field or tuple position `segment` of a value of `ty`.
    pub(crate) fn project_type(&self, ty: &Type, segment: &str) -> Option<Type> {
        match ty {
            Type::Union(union) => union
                .members
                .get(exec::union_payload_index(segment)?)
                .cloned(),
            Type::Tuple(items) => items.get(segment.parse::<usize>().ok()?).cloned(),
            Type::Named(name, args) => self
                .class_fields(name, args)?
                .into_iter()
                .find(|(field, _)| field == segment)
                .map(|(_, ty)| ty),
            _ => None,
        }
    }
}

fn substitute(ty: &Type, substitutions: &BTreeMap<&str, &Type>) -> Type {
    match ty {
        Type::TypeParam(name) => substitutions
            .get(name.as_str())
            .map(|ty| (*ty).clone())
            .unwrap_or_else(|| ty.clone()),
        Type::Named(name, args) => Type::Named(
            name.clone(),
            args.iter()
                .map(|arg| substitute(arg, substitutions))
                .collect(),
        ),
        Type::Tuple(items) => Type::Tuple(
            items
                .iter()
                .map(|item| substitute(item, substitutions))
                .collect(),
        ),
        _ => ty.clone(),
    }
}

/// Whether `function` is one the report covers: a function of the entry
/// module that is not a `spec def`.
fn verified_function(program: &Program, cx: &VerifyContext<'_>, function: &MirFunction) -> bool {
    if function.module_name != program.module_name {
        return false;
    }
    if function.name.contains("::__default_") {
        return false;
    }
    // A class's invariant check runs where the invariant must hold, and
    // each of its callers proves it there.
    if spec::base_name(&function.name)
        .ends_with(&format!(".{}", crate::ast::CLASS_INVARIANT_METHOD))
    {
        return false;
    }
    !cx.decl(&function.name).is_some_and(|decl| decl.spec)
}

/// Verifies every function of `program` whose MIR is `module`.
pub fn verify_module(
    program: &Program,
    module: &MirModule,
    options: &Options,
) -> std::result::Result<Report, Diagnostic> {
    let command = smt::SolverCommand::find(options.solver.as_deref())
        .map_err(|message| Diagnostic::coded("AU5103", message))?;
    let cx = VerifyContext::new(program, module);
    let version = command.version();
    let mut report = Report {
        solver: format!("{} ({version})", command.program.display()),
        ..Report::default()
    };
    let mut merged: BTreeMap<((usize, usize), ObligationKind, String), ObligationReport> =
        BTreeMap::new();
    // The solver starts at the first function the cache cannot answer.
    let mut solver: Option<smt::Solver> = None;
    let trace = options.trace;
    for function in &module.functions {
        if !verified_function(program, &cx, function) {
            continue;
        }
        if let Some(filter) = &options.function_filter {
            if !function.name.contains(filter.as_str()) {
                continue;
            }
        }
        report.functions += 1;
        let public = crate::mir::public_function_name(&function.name);
        let mut exec = exec::Exec::new(&cx, function);
        let entry = exec.entry_state();
        exec.assume_entry_requires(&entry);
        exec.assume_entry_invariant(&entry);
        exec.run(entry);
        if let Some(reason) = exec.unsupported.clone() {
            let key = (
                (function.span.line, function.span.column),
                ObligationKind::Assert,
                "function body".to_string(),
            );
            merged.insert(
                key,
                ObligationReport {
                    kind: ObligationKind::Assert,
                    function: public.clone(),
                    span: function.span,
                    message: "the function body".to_string(),
                    status: Status::Unsupported(reason),
                },
            );
            continue;
        }
        let cache = options
            .cache_dir
            .as_deref()
            .map(|dir| dir.join(format!("{}.json", cache_key(&version, options, &exec))));
        let statuses = match cache.as_deref().and_then(|path| read_cache(path, &exec)) {
            Some(statuses) => {
                report.cached += 1;
                statuses
            }
            None => {
                let solver = match &mut solver {
                    Some(solver) => solver,
                    None => solver.insert(
                        smt::Solver::start(&command, options.timeout_ms)
                            .map_err(|message| Diagnostic::coded("AU5103", message))?,
                    ),
                };
                let statuses = solve(solver, &exec, trace)
                    .map_err(|message| Diagnostic::coded("AU5103", message))?;
                if let Some(path) = &cache {
                    write_cache(path, &statuses);
                }
                statuses
            }
        };
        for (obligation, status) in exec.obligations.iter().zip(statuses) {
            let key = (
                (obligation.span.line, obligation.span.column),
                obligation.kind,
                obligation.message.clone(),
            );
            match merged.get_mut(&key) {
                Some(existing) => {
                    if status.rank() > existing.status.rank() {
                        existing.status = status;
                    }
                }
                None => {
                    merged.insert(
                        key,
                        ObligationReport {
                            kind: obligation.kind,
                            function: public.clone(),
                            span: obligation.span,
                            message: obligation.message.clone(),
                            status,
                        },
                    );
                }
            }
        }
    }
    report.obligations = merged.into_values().collect();
    Ok(report)
}

/// The proof cache key of a function's queries: everything the solver
/// sees, the solver's version, and the time limit.
fn cache_key(version: &str, options: &Options, exec: &exec::Exec<'_>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut part = |text: &str| {
        hasher.update(text.len().to_le_bytes());
        hasher.update(text.as_bytes());
    };
    part("aura-verify-cache-1");
    part(version);
    part(&options.timeout_ms.to_string());
    for decl in &exec.decls {
        part(decl);
    }
    part("facts");
    for fact in &exec.facts {
        part(fact);
    }
    part("inputs");
    for (name, term) in &exec.inputs {
        part(name);
        part(term);
    }
    for obligation in &exec.obligations {
        part(&obligation.facts.to_string());
        part(&obligation.pc);
        match &obligation.goal {
            exec::Goal::Holds(goal) => part(goal),
            exec::Goal::Unsupported(reason) => {
                part("unsupported");
                part(reason);
            }
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The cached statuses at `path`, when there is one for every obligation.
fn read_cache(path: &std::path::Path, exec: &exec::Exec<'_>) -> Option<Vec<Status>> {
    let text = std::fs::read_to_string(path).ok()?;
    let statuses: Vec<Status> = serde_json::from_str(&text).ok()?;
    (statuses.len() == exec.obligations.len()).then_some(statuses)
}

/// Records decided statuses. An answer the solver gave up on depends on the
/// machine's speed, so it is solved again next time. Failing to write only
/// loses the cache entry.
fn write_cache(path: &std::path::Path, statuses: &[Status]) {
    if statuses.contains(&Status::Unknown) {
        return;
    }
    let text = serde_json::to_string(statuses).expect("statuses serialize");
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    let written = std::fs::create_dir_all(path.parent().expect("a cache file has a directory"))
        .and_then(|()| std::fs::write(&temp, text))
        .and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
}

/// Checks each obligation of `exec` against its declarations and facts.
fn solve(
    solver: &mut smt::Solver,
    exec: &exec::Exec<'_>,
    trace: bool,
) -> std::result::Result<Vec<Status>, String> {
    let mut prelude = String::from("(push 1)\n");
    for decl in &exec.decls {
        prelude.push_str(decl);
        prelude.push('\n');
    }
    if trace {
        eprintln!("; function {}\n{prelude}", exec.function.name);
    }
    solver.send(&prelude)?;
    let mut statuses = Vec::new();
    let mut asserted = 0usize;
    for obligation in &exec.obligations {
        // Facts are asserted in the order the walk assumed them, so each
        // check sees only what holds before it.
        if obligation.facts > asserted {
            let mut text = String::new();
            for fact in &exec.facts[asserted..obligation.facts] {
                text.push_str(&format!("(assert {fact})\n"));
            }
            if trace {
                eprintln!("{text}");
            }
            solver.send(&text)?;
            asserted = obligation.facts;
        }
        let goal = match &obligation.goal {
            exec::Goal::Unsupported(reason) => {
                statuses.push(Status::Unsupported(reason.clone()));
                continue;
            }
            exec::Goal::Holds(goal) => goal,
        };
        let query = format!(
            "(push 1)\n(assert {})\n",
            exec::and(&obligation.pc, &exec::not(goal))
        );
        if trace {
            eprintln!(
                "; {} at {}:{}\n{query}",
                obligation.message, obligation.span.line, obligation.span.column
            );
        }
        solver.send(&query)?;
        let status = match solver.check()? {
            smt::SatResult::Unsat => Status::Proved,
            smt::SatResult::Unknown => Status::Unknown,
            smt::SatResult::Sat => {
                let terms: Vec<String> = exec.inputs.iter().map(|(_, term)| term.clone()).collect();
                let counterexample = solver
                    .values(&terms)
                    .map(|values| {
                        exec.inputs
                            .iter()
                            .zip(values)
                            .map(|((name, _), value)| (name.clone(), smt::render_int(&value)))
                            .collect()
                    })
                    .unwrap_or_default();
                Status::Failed { counterexample }
            }
        };
        if trace {
            eprintln!("; => {status:?}");
        }
        statuses.push(status);
        solver.send("(pop 1)")?;
    }
    solver.send("(pop 1)")?;
    Ok(statuses)
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
