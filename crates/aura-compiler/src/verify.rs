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
pub(crate) use exec::CHECK_FAILURE_SITE;
#[path = "verify_float.rs"]
mod float;
#[path = "verify_smt.rs"]
pub mod smt;
#[path = "verify_spec.rs"]
mod spec;

use std::collections::{BTreeMap, BTreeSet};

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
    /// A `dict` key read with brackets, or a `set` element removed, is
    /// there.
    KeyPresent,
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
            Self::KeyPresent => "key present",
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
    /// With [`Options::prove_sites`], the element accesses proved to need
    /// no bounds check, by MIR function name, block, and position.
    pub proved_sites: BTreeSet<(String, usize, usize)>,
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
    /// Also prove which element accesses can skip their bounds check, for
    /// a build that removes proved checks.
    pub prove_sites: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            solver: None,
            timeout_ms: 10_000,
            function_filter: None,
            trace: std::env::var_os("AURA_VERIFY_TRACE").is_some(),
            cache_dir: None,
            prove_sites: false,
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
    extern_decls: BTreeMap<String, &'a FunctionDecl>,
    spec_defs: BTreeMap<String, &'a FunctionDecl>,
    decl_modules_by_ptr: BTreeMap<usize, &'a str>,
    spec_return_by_ptr: BTreeMap<usize, &'a Type>,
    class_invariants: BTreeMap<String, (&'a str, Vec<ContractClause>)>,
    module_paths: BTreeSet<String>,
    module_aliases: BTreeMap<(String, String), String>,
    /// Each trait method's declaration, by `Trait.method`.
    trait_methods: BTreeMap<String, &'a FunctionDecl>,
    /// The trait method each implementation method implements, by MIR
    /// function name.
    implemented: BTreeMap<String, &'a FunctionDecl>,
    /// The contract of each callable alias that states one, by name.
    callable_contracts: BTreeMap<String, &'a crate::sema::FunctionInfo>,
    /// The callable alias with a contract that each class field declared
    /// as one has, by class and field.
    field_contracts: BTreeMap<(String, String), String>,
}

impl<'a> VerifyContext<'a> {
    fn new(program: &'a Program, module: &'a MirModule) -> Self {
        let mut decls = BTreeMap::new();
        let mut extern_decls = BTreeMap::new();
        let mut spec_defs = BTreeMap::new();
        let mut decl_modules_by_ptr = BTreeMap::new();
        let mut spec_return_by_ptr = BTreeMap::new();
        let mut callable_contracts = BTreeMap::new();
        let mut class_invariants = BTreeMap::new();
        let mut field_contracts = BTreeMap::new();
        let mut trait_methods = BTreeMap::new();
        let mut impl_methods = BTreeMap::new();
        let mut module_paths = BTreeSet::new();
        let mut module_aliases = BTreeMap::new();

        for (alias, ns) in &program.imported_modules {
            module_aliases.insert(
                (program.module_name.clone(), alias.clone()),
                ns.path.clone(),
            );
        }

        for function in program.extern_functions.values() {
            if let Some(decl) = &function.contract_decl {
                decl_modules_by_ptr.insert(
                    std::ptr::from_ref(decl) as usize,
                    function.module_name.as_str(),
                );
                decls.entry(function.decl.name.clone()).or_insert(decl);
                if function.module_name != program.module_name {
                    decls
                        .entry(format!("{}::{}", function.module_name, function.decl.name))
                        .or_insert(decl);
                }
                extern_decls
                    .entry(function.decl.link_name().to_string())
                    .or_insert(decl);
            }
        }
        for (name, function) in &program.functions {
            decl_modules_by_ptr.insert(
                std::ptr::from_ref(&function.decl) as usize,
                function.module_name.as_str(),
            );
            decls.insert(name.clone(), &function.decl);
            if function.module_name != program.module_name {
                decls.insert(
                    format!("{}::{}", function.module_name, function.decl.name),
                    &function.decl,
                );
            }
            if function.decl.spec {
                spec_return_by_ptr.insert(
                    std::ptr::from_ref(&function.decl) as usize,
                    &function.signature.return_type,
                );
                spec_defs.insert(name.clone(), &function.decl);
                spec_defs.insert(format!("{}::{name}", program.module_name), &function.decl);
                if function.module_name != program.module_name {
                    spec_defs.insert(
                        format!("{}::{}", function.module_name, function.decl.name),
                        &function.decl,
                    );
                }
            }
        }
        for (name, alias) in program.aliases_in_scope() {
            if let Some(info) = &alias.contract {
                decl_modules_by_ptr.insert(
                    std::ptr::from_ref(&info.decl) as usize,
                    info.module_name.as_str(),
                );
                callable_contracts.insert(name.clone(), info);
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn register_class<'a>(
            root_module_name: &str,
            class: &'a crate::sema::ClassInfo,
            decls: &mut BTreeMap<String, &'a FunctionDecl>,
            spec_defs: &mut BTreeMap<String, &'a FunctionDecl>,
            decl_modules_by_ptr: &mut BTreeMap<usize, &'a str>,
            spec_return_by_ptr: &mut BTreeMap<usize, &'a Type>,
            class_invariants: &mut BTreeMap<String, (&'a str, Vec<ContractClause>)>,
            field_contracts: &mut BTreeMap<(String, String), String>,
            callable_contracts: &BTreeMap<String, &'a crate::sema::FunctionInfo>,
        ) {
            let class_key = if class.module_name == root_module_name {
                class.decl.name.clone()
            } else {
                format!("{}.{}", class.module_name, class.decl.name)
            };
            let mir_prefix = if class.module_name == root_module_name {
                class.decl.name.clone()
            } else {
                format!("{}::{}", class.module_name, class.decl.name)
            };
            for field in &class.decl.fields {
                if let crate::ast::TypeRefKind::Named { name, args } = &field.ty.kind {
                    let qualified_alias = format!("{}::{name}", class.module_name);
                    let resolved_alias = if callable_contracts.contains_key(&qualified_alias) {
                        Some(qualified_alias)
                    } else if callable_contracts.contains_key(name) {
                        Some(name.clone())
                    } else {
                        None
                    };
                    if args.is_empty() {
                        if let Some(alias_key) = resolved_alias {
                            field_contracts
                                .entry((class_key.clone(), field.name.clone()))
                                .or_insert_with(|| alias_key.clone());
                            field_contracts
                                .entry((class.decl.name.clone(), field.name.clone()))
                                .or_insert(alias_key);
                        }
                    }
                }
            }
            for (name, method) in &class.methods {
                decl_modules_by_ptr.insert(
                    std::ptr::from_ref(&method.decl) as usize,
                    class.module_name.as_str(),
                );
                decls
                    .entry(format!("{mir_prefix}.{name}"))
                    .or_insert(&method.decl);
                if class.module_name == root_module_name {
                    decls.insert(format!("{}.{name}", class.decl.name), &method.decl);
                }
                if method.decl.spec {
                    spec_return_by_ptr.insert(
                        std::ptr::from_ref(&method.decl) as usize,
                        &method.signature.return_type,
                    );
                    spec_defs
                        .entry(format!("{class_key}.{name}"))
                        .or_insert(&method.decl);
                    spec_defs
                        .entry(format!("{mir_prefix}.{name}"))
                        .or_insert(&method.decl);
                }
                if name == crate::ast::CLASS_INVARIANT_METHOD {
                    let clauses: Vec<ContractClause> = method
                        .decl
                        .contracts
                        .iter()
                        .filter(|clause| clause.kind == ContractKind::Invariant)
                        .cloned()
                        .collect();
                    class_invariants
                        .entry(class_key.clone())
                        .or_insert((class.module_name.as_str(), clauses));
                }
            }
        }

        for class in program.classes.values() {
            register_class(
                &program.module_name,
                class,
                &mut decls,
                &mut spec_defs,
                &mut decl_modules_by_ptr,
                &mut spec_return_by_ptr,
                &mut class_invariants,
                &mut field_contracts,
                &callable_contracts,
            );
        }
        // A method of a trait implementation has the contract of its
        // declaration, found by its span: the implementation's own method,
        // or the trait's default.
        for (trait_name, info) in &program.traits {
            for (method, method_info) in &info.methods {
                decl_modules_by_ptr.insert(
                    std::ptr::from_ref(&method_info.decl) as usize,
                    info.module_name.as_str(),
                );
                trait_methods.insert(format!("{trait_name}.{method}"), &method_info.decl);
            }
        }
        for trait_impl in &program.trait_impls {
            for (method_name, method) in &trait_impl.methods {
                decl_modules_by_ptr.insert(
                    std::ptr::from_ref(&method.decl) as usize,
                    trait_impl.module_name.as_str(),
                );
                let trait_method = program
                    .traits
                    .get(&trait_impl.trait_name)
                    .and_then(|info| info.methods.get(method_name))
                    .map(|info| &info.decl);
                impl_methods.insert(
                    (
                        trait_impl.module_name.as_str(),
                        method.decl.span.line,
                        method.decl.span.column,
                    ),
                    (&method.decl, trait_method),
                );
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn visit_namespace<'a>(
            program: &'a Program,
            ns: &'a crate::sema::ModuleNamespace,
            visited: &mut BTreeSet<(String, usize)>,
            module_paths: &mut BTreeSet<String>,
            module_aliases: &mut BTreeMap<(String, String), String>,
            decls: &mut BTreeMap<String, &'a FunctionDecl>,
            extern_decls: &mut BTreeMap<String, &'a FunctionDecl>,
            spec_defs: &mut BTreeMap<String, &'a FunctionDecl>,
            decl_modules_by_ptr: &mut BTreeMap<usize, &'a str>,
            spec_return_by_ptr: &mut BTreeMap<usize, &'a Type>,
            callable_contracts: &mut BTreeMap<String, &'a crate::sema::FunctionInfo>,
            class_invariants: &mut BTreeMap<String, (&'a str, Vec<ContractClause>)>,
            field_contracts: &mut BTreeMap<(String, String), String>,
            trait_methods: &mut BTreeMap<String, &'a FunctionDecl>,
            impl_methods: &mut BTreeMap<
                (&'a str, usize, usize),
                (&'a FunctionDecl, Option<&'a FunctionDecl>),
            >,
        ) {
            if !visited.insert((ns.path.clone(), std::ptr::from_ref(ns) as usize)) {
                return;
            }
            module_paths.insert(ns.path.clone());
            for (alias, child) in ns.imported_modules.iter().chain(&ns.modules) {
                module_aliases.insert((ns.path.clone(), alias.clone()), child.path.clone());
            }
            for (name, alias) in ns.all_aliases.iter().chain(&ns.aliases) {
                if let Some(info) = &alias.contract {
                    decl_modules_by_ptr.insert(
                        std::ptr::from_ref(&info.decl) as usize,
                        info.module_name.as_str(),
                    );
                    callable_contracts
                        .entry(format!("{}::{name}", ns.path))
                        .or_insert(info);
                    callable_contracts.entry(name.clone()).or_insert(info);
                }
            }
            for extern_fn in ns
                .extern_functions
                .values()
                .chain(ns.all_extern_functions.values())
            {
                if let Some(decl) = &extern_fn.contract_decl {
                    decl_modules_by_ptr.insert(
                        std::ptr::from_ref(decl) as usize,
                        extern_fn.module_name.as_str(),
                    );
                    decls
                        .entry(format!(
                            "{}::{}",
                            extern_fn.module_name, extern_fn.decl.name
                        ))
                        .or_insert(decl);
                    extern_decls
                        .entry(extern_fn.decl.link_name().to_string())
                        .or_insert(decl);
                }
            }
            for (local_name, function) in ns.all_functions.iter().chain(&ns.functions) {
                decl_modules_by_ptr.insert(
                    std::ptr::from_ref(&function.decl) as usize,
                    function.module_name.as_str(),
                );
                decls
                    .entry(format!("{}::{}", function.module_name, function.decl.name))
                    .or_insert(&function.decl);
                if function.decl.spec {
                    spec_return_by_ptr.insert(
                        std::ptr::from_ref(&function.decl) as usize,
                        &function.signature.return_type,
                    );
                    spec_defs
                        .entry(format!("{}::{}", function.module_name, function.decl.name))
                        .or_insert(&function.decl);
                    spec_defs
                        .entry(format!("{}::{local_name}", ns.path))
                        .or_insert(&function.decl);
                }
            }
            for class in ns.all_classes.values().chain(ns.classes.values()) {
                register_class(
                    &program.module_name,
                    class,
                    decls,
                    spec_defs,
                    decl_modules_by_ptr,
                    spec_return_by_ptr,
                    class_invariants,
                    field_contracts,
                    callable_contracts,
                );
            }
            for (trait_name, info) in ns.all_traits.iter().chain(&ns.traits) {
                for (method, method_info) in &info.methods {
                    decl_modules_by_ptr.insert(
                        std::ptr::from_ref(&method_info.decl) as usize,
                        info.module_name.as_str(),
                    );
                    trait_methods
                        .entry(format!("{trait_name}.{method}"))
                        .or_insert(&method_info.decl);
                    trait_methods
                        .entry(format!("{}.{}.{method}", info.module_name, info.decl.name))
                        .or_insert(&method_info.decl);
                }
            }
            for trait_impl in &ns.trait_impls {
                for (method_name, method) in &trait_impl.methods {
                    decl_modules_by_ptr.insert(
                        std::ptr::from_ref(&method.decl) as usize,
                        trait_impl.module_name.as_str(),
                    );
                    let short_trait = trait_impl
                        .trait_name
                        .rsplit('.')
                        .next()
                        .unwrap_or(&trait_impl.trait_name);
                    let trait_method = ns
                        .all_traits
                        .get(&trait_impl.trait_name)
                        .or_else(|| ns.traits.get(&trait_impl.trait_name))
                        .or_else(|| ns.all_traits.get(short_trait))
                        .or_else(|| ns.traits.get(short_trait))
                        .or_else(|| program.traits.get(&trait_impl.trait_name))
                        .and_then(|info| info.methods.get(method_name))
                        .map(|info| &info.decl);
                    impl_methods
                        .entry((
                            trait_impl.module_name.as_str(),
                            method.decl.span.line,
                            method.decl.span.column,
                        ))
                        .or_insert((&method.decl, trait_method));
                }
            }
            for child in ns.modules.values().chain(ns.imported_modules.values()) {
                visit_namespace(
                    program,
                    child,
                    visited,
                    module_paths,
                    module_aliases,
                    decls,
                    extern_decls,
                    spec_defs,
                    decl_modules_by_ptr,
                    spec_return_by_ptr,
                    callable_contracts,
                    class_invariants,
                    field_contracts,
                    trait_methods,
                    impl_methods,
                );
            }
        }

        let mut visited = BTreeSet::new();
        for ns in program
            .module_registry
            .values()
            .chain(program.imported_modules.values())
        {
            visit_namespace(
                program,
                ns,
                &mut visited,
                &mut module_paths,
                &mut module_aliases,
                &mut decls,
                &mut extern_decls,
                &mut spec_defs,
                &mut decl_modules_by_ptr,
                &mut spec_return_by_ptr,
                &mut callable_contracts,
                &mut class_invariants,
                &mut field_contracts,
                &mut trait_methods,
                &mut impl_methods,
            );
        }

        let mut implemented = BTreeMap::new();
        for function in &module.functions {
            let name = spec::base_name(&function.name);
            if name.contains(" for ") {
                if let Some((decl, trait_method)) = impl_methods.get(&(
                    function.module_name.as_str(),
                    function.span.line,
                    function.span.column,
                )) {
                    decls.entry(name.to_string()).or_insert(decl);
                    if let Some(trait_method) = trait_method {
                        implemented.insert(name.to_string(), *trait_method);
                    }
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
            extern_decls,
            spec_defs,
            decl_modules_by_ptr,
            spec_return_by_ptr,
            class_invariants,
            module_paths,
            module_aliases,
            trait_methods,
            implemented,
            callable_contracts,
            field_contracts,
        }
    }

    /// The callable alias with a contract that field `field` of class
    /// `class` is declared as.
    pub(crate) fn field_contract(&self, class: &str, field: &str) -> Option<String> {
        let normalized = class.replace("::", ".");
        if let Some(found) = self
            .field_contracts
            .get(&(normalized, field.to_string()))
            .cloned()
        {
            return Some(found);
        }
        let class = class.rsplit("::").next().unwrap_or(class);
        self.field_contracts
            .get(&(class.to_string(), field.to_string()))
            .cloned()
    }

    /// The callable alias with a contract that function `name` is declared
    /// to return.
    pub(crate) fn return_contract(&self, name: &str) -> Option<String> {
        self.decl(name)
            .and_then(|decl| self.callable_contract(&decl.return_type))
            .map(|(alias, _)| alias)
    }

    /// The declaration of trait method `method` of `trait_name`.
    pub(crate) fn trait_method(&self, trait_name: &str, method: &str) -> Option<&'a FunctionDecl> {
        let normalized = trait_name.replace("::", ".");
        if let Some(decl) = self.trait_methods.get(&format!("{normalized}.{method}")) {
            return Some(*decl);
        }
        let short = trait_name.rsplit("::").next().unwrap_or(trait_name);
        self.trait_methods
            .get(&format!("{short}.{method}"))
            .copied()
    }

    /// The trait method the implementation method `name` implements.
    pub(crate) fn implemented(&self, name: &str) -> Option<&'a FunctionDecl> {
        self.implemented.get(spec::base_name(name)).copied()
    }

    pub(crate) fn function(&self, name: &str) -> Option<&'a MirFunction> {
        self.functions.get(name).copied()
    }

    /// The declaration whose contract a call of MIR function `name` uses.
    pub(crate) fn decl(&self, name: &str) -> Option<&'a FunctionDecl> {
        self.decls.get(spec::base_name(name)).copied()
    }

    /// The contract declaration of an `extern` function by its C link symbol.
    pub(crate) fn extern_decl(&self, symbol: &str) -> Option<&'a FunctionDecl> {
        self.extern_decls.get(symbol).copied()
    }

    /// The module where `decl` was written, when recorded.
    pub(crate) fn decl_module(&self, decl: &FunctionDecl) -> Option<&'a str> {
        self.decl_modules_by_ptr
            .get(&(std::ptr::from_ref(decl) as usize))
            .copied()
    }

    /// The return type of `spec`, when `spec` is a `spec def`.
    pub(crate) fn spec_return_type(&self, spec: &FunctionDecl) -> Option<&'a Type> {
        self.spec_return_by_ptr
            .get(&(std::ptr::from_ref(spec) as usize))
            .copied()
    }

    /// Resolves `dotted` as an imported module path inside `current_module`.
    pub(crate) fn resolve_module_path(&self, current_module: &str, dotted: &str) -> Option<String> {
        if let Some(path) = self
            .module_aliases
            .get(&(current_module.to_string(), dotted.to_string()))
        {
            return Some(path.clone());
        }
        if let Some((head, tail)) = dotted.split_once('.') {
            if let Some(prefix) = self
                .module_aliases
                .get(&(current_module.to_string(), head.to_string()))
            {
                let candidate = format!("{prefix}.{tail}");
                if self.module_paths.contains(&candidate) {
                    return Some(candidate);
                }
            }
        }
        self.module_paths
            .contains(dotted)
            .then(|| dotted.to_string())
    }

    /// The alias and its contract, when `ty` names a callable alias that
    /// states one.
    pub(crate) fn callable_contract(
        &self,
        ty: &crate::ast::TypeRef,
    ) -> Option<(String, &'a crate::sema::FunctionInfo)> {
        let crate::ast::TypeRefKind::Named { name, args } = &ty.kind else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        self.callable_contracts
            .get(name)
            .map(|info| (name.clone(), *info))
    }

    /// The contract of the callable alias `name`.
    pub(crate) fn alias_contract(&self, name: &str) -> Option<&'a crate::sema::FunctionInfo> {
        self.callable_contracts.get(name).copied()
    }

    pub(crate) fn spec_def(&self, name: &str) -> Option<&'a FunctionDecl> {
        self.spec_defs.get(spec::base_name(name)).copied()
    }

    pub(crate) fn spec_def_in(&self, module_name: &str, name: &str) -> Option<&'a FunctionDecl> {
        let base = spec::base_name(name);
        self.spec_defs
            .get(&format!("{module_name}::{base}"))
            .copied()
            .or_else(|| self.spec_defs.get(base).copied())
    }

    pub(crate) fn class_invariant(&self, class: &str) -> Option<(&'a str, &[ContractClause])> {
        let normalized = class.replace("::", ".");
        self.class_invariants
            .get(&normalized)
            .or_else(|| self.class_invariants.get(class))
            .map(|(module_name, clauses)| (*module_name, clauses.as_slice()))
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
        // The builtin `Lookup` a checked list read returns.
        if name == LOOKUP && !self.enums.contains_key(name) {
            return ["Found", "Missing"]
                .iter()
                .position(|candidate| *candidate == variant);
        }
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

/// Default-argument thunks in the entry module and imported `lemma def`
/// bodies are also checked so an ill-defined default or an unproved imported
/// lemma cannot go unnoticed.
fn auxiliary_verified_function(
    program: &Program,
    cx: &VerifyContext<'_>,
    function: &MirFunction,
) -> bool {
    if function.module_name == program.module_name {
        if let Some((owner, _)) = function.name.split_once("::__default_") {
            return !cx.decl(owner).is_some_and(|decl| decl.spec);
        }
        return false;
    }
    !function.name.contains("::__default_")
        && cx.decl(&function.name).is_some_and(|decl| decl.lemma)
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
    let mut merged: BTreeMap<(String, (usize, usize), ObligationKind, String), ObligationReport> =
        BTreeMap::new();
    let mut proved_sites = BTreeSet::new();
    let mut erased_open = false;
    // The solver starts at the first function the cache cannot answer.
    let mut solver: Option<smt::Solver> = None;
    let mut rough_solver: Option<smt::Solver> = None;
    let mut sample_solver: Option<smt::Solver> = None;
    let trace = options.trace;
    for function in &module.functions {
        let is_primary = verified_function(program, &cx, function);
        let is_auxiliary = !is_primary && auxiliary_verified_function(program, &cx, function);
        if !is_primary && !is_auxiliary {
            continue;
        }
        if let Some(filter) = &options.function_filter {
            if !function.name.contains(filter.as_str()) {
                continue;
            }
        }
        if is_primary {
            report.functions += 1;
        }
        let public = crate::mir::public_function_name(&function.name);
        let module_order = if function.module_name == program.module_name {
            String::new()
        } else {
            function.module_name.clone()
        };
        let mut exec = exec::Exec::new(&cx, function);
        exec.prove_sites = options.prove_sites;
        let entry = exec.entry_state();
        exec.prove_weakened_requires(&entry);
        exec.assume_entry_requires(&entry);
        exec.assume_entry_invariant(&entry);
        exec.assume_entry_parameters(&entry);
        exec.run(entry);
        if let Some(reason) = exec.unsupported.clone() {
            if !is_primary {
                erased_open |= cx.decl(&function.name).is_some_and(|decl| decl.lemma);
            }
            let key = (
                module_order,
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
                if is_primary {
                    report.cached += 1;
                }
                statuses
            }
            None => {
                let start = || smt::Solver::start(&command, options.timeout_ms);
                let rough_start =
                    || smt::Solver::start(&command, options.timeout_ms.min(ROUGH_TIMEOUT_MS));
                let main_lane = Lane::new(&mut solver, &start);
                let rough = if exec.rounded.is_empty() {
                    None
                } else {
                    Some(Lane::new(&mut rough_solver, &rough_start))
                };
                let sample_start =
                    || smt::Solver::start(&command, options.timeout_ms.min(SAMPLE_TIMEOUT_MS));
                let sample = if exec.rounded.is_empty() {
                    None
                } else {
                    Some(Lane::new(&mut sample_solver, &sample_start))
                };
                let statuses = solve(main_lane, rough, sample, &exec, trace)
                    .map_err(|message| Diagnostic::coded("AU5103", message))?;
                if let Some(path) = &cache {
                    write_cache(path, &statuses);
                }
                statuses
            }
        };
        // A site is proved only when every path that reaches it is.
        let mut sites: BTreeMap<(usize, usize), bool> = BTreeMap::new();
        for (obligation, status) in exec.obligations.iter().zip(statuses) {
            erased_open |= obligation.erased && status != Status::Proved;
            if let Some(site) = obligation.site {
                *sites.entry(site).or_insert(true) &= status == Status::Proved;
                continue;
            }
            if !is_primary && status == Status::Proved {
                continue;
            }
            let key = (
                module_order.clone(),
                (obligation.span.line, obligation.span.column),
                obligation.kind,
                obligation.message.clone(),
            );
            match merged.get_mut(&key) {
                Some(existing) => {
                    if status.rank() > existing.status.rank() {
                        existing.status = status;
                        existing.function = public.clone();
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
        proved_sites.extend(
            sites
                .into_iter()
                .filter(|(_, proved)| *proved)
                .map(|((block, position), _)| (function.name.clone(), block, position)),
        );
    }
    report.obligations = merged.into_values().collect();
    // A lemma that is not proved may have let the verifier assume what no
    // run checks, so no access counts as proved.
    if !erased_open {
        report.proved_sites = proved_sites;
    }
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
    part("aura-verify-cache-2");
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

/// The time limit of the first, weaker query of a check that reads a float
/// `/` or `sqrt` (see `verify_float.rs`).
const ROUGH_TIMEOUT_MS: u64 = 2_000;

/// The time limit of a check asked with each float input fixed to a
/// sampled value.
const SAMPLE_TIMEOUT_MS: u64 = 2_000;

/// How many checks of one function can be tried with every sample.
const SAMPLE_CHECKS: usize = 8;

/// The first `float::SAMPLES` of `candidates` that the facts on entry
/// allow, so each one tried against a check is an input the function can
/// have. `None` when the solver could not decide one in time.
fn allowed_samples(
    lane: &mut Lane<'_>,
    exec: &exec::Exec<'_>,
    candidates: Vec<String>,
    trace: bool,
) -> std::result::Result<Option<Vec<String>>, String> {
    let mut entry = String::from("; inputs the function can have\n");
    for fact in &exec.facts[..exec.entry_facts] {
        entry.push_str(&format!("(assert {fact})\n"));
    }
    let mut allowed = Vec::new();
    let mut tried = 0usize;
    for fixed in candidates {
        if allowed.len() == float::SAMPLES {
            break;
        }
        tried += 1;
        match lane.ask(&format!("(push 1)\n{fixed}{entry}"), None)? {
            Status::Failed { .. } => allowed.push(fixed),
            Status::Proved => {}
            _ => return Ok(None),
        }
    }
    if trace {
        eprintln!(
            "; {} of {tried} candidate float inputs are allowed on entry",
            allowed.len()
        );
    }
    Ok(Some(allowed))
}

/// The declarations of `exec` with each float input fixed to a value, one
/// text for each row of candidate values. Empty when no input is a float.
///
/// A fixed input is defined, not declared and constrained, so the solver
/// computes every operation on it while it reads the problem.
fn sample_preludes(exec: &exec::Exec<'_>, literals: &[f64]) -> Vec<String> {
    let mut fixed: Vec<(usize, &str, exec::FloatTy)> = Vec::new();
    for (_, term) in &exec.inputs {
        for float in [exec::FloatTy::F32, exec::FloatTy::F64] {
            let decl = format!("(declare-const {term} {})", float.sort());
            if let Some(at) = exec.decls.iter().position(|existing| *existing == decl) {
                if !fixed.iter().any(|(index, _, _)| *index == at) {
                    fixed.push((at, term, float));
                }
            }
        }
    }
    if fixed.is_empty() {
        return Vec::new();
    }
    float::candidates(fixed.len(), literals)
        .into_iter()
        .map(|row| {
            let mut text = String::new();
            for (index, decl) in exec.decls.iter().enumerate() {
                match fixed.iter().position(|(at, _, _)| *at == index) {
                    Some(slot) => {
                        let (_, term, float) = fixed[slot];
                        text.push_str(&format!(
                            "(define-fun {term} () {} {})\n",
                            float.sort(),
                            float::literal(float, row[slot])
                        ));
                    }
                    None => {
                        text.push_str(decl);
                        text.push('\n');
                    }
                }
            }
            text
        })
        .collect()
}

/// A solver process for one function's queries, with everything sent at
/// the function's own scope kept so a fresh process can be brought to the
/// same point.
///
/// Z3 does not always survive a query that stops at its time limit: later
/// queries in the same process have crashed it (SIGSEGV) or had a `push`
/// canceled. So a solver that answered `unknown` is not reused, and the
/// next query runs in a fresh one. A solver that dies mid-query gets one
/// fresh retry of that query; a second death is reported, so a solver that
/// always crashes still fails the run. One that dies between queries, after
/// an answer, is replaced at the next thing sent.
///
/// A solver counts as dead when a write to it or a read from it fails, not
/// when its process is seen to have exited: the pipe closes first, and how
/// long the exit then takes is the system's business (a crash handler can
/// hold the process for seconds).
struct Lane<'a> {
    slot: &'a mut Option<smt::Solver>,
    start: &'a dyn Fn() -> std::result::Result<smt::Solver, String>,
    base: String,
}

impl<'a> Lane<'a> {
    fn new(
        slot: &'a mut Option<smt::Solver>,
        start: &'a dyn Fn() -> std::result::Result<smt::Solver, String>,
    ) -> Self {
        Self {
            slot,
            start,
            base: String::new(),
        }
    }

    /// The running solver, started and given the function's scope so far
    /// when there is none.
    fn solver(&mut self) -> std::result::Result<&mut smt::Solver, String> {
        if self.slot.is_none() {
            let mut fresh = (self.start)()?;
            if !self.base.is_empty() {
                fresh.send(&self.base)?;
            }
            *self.slot = Some(fresh);
        }
        Ok(self.slot.as_mut().expect("a solver was started above"))
    }

    /// Sends `text` at the function's scope. A solver found dead here, after
    /// its last answer, is replaced by a fresh one.
    fn send(&mut self, text: &str) -> std::result::Result<(), String> {
        self.base.push_str(text);
        self.base.push('\n');
        if let Some(solver) = self.slot.as_mut() {
            if solver.send(text).is_ok() {
                return Ok(());
            }
            *self.slot = None;
        }
        self.solver().map(|_| ())
    }

    /// Asserts `query` (a `(push 1)` and the negated goal) and asks whether
    /// it is satisfiable, with the model's values of `exec`'s inputs after
    /// `sat` when `exec` is given. The query's scope is closed after.
    fn ask(
        &mut self,
        query: &str,
        exec: Option<&exec::Exec<'_>>,
    ) -> std::result::Result<Status, String> {
        let mut retried = false;
        let status = loop {
            let solver = self.solver()?;
            match ask_once(solver, query, exec) {
                Ok(status) => break status,
                Err(_) if !retried && solver.stopped() => {
                    *self.slot = None;
                    retried = true;
                }
                Err(message) => return Err(message),
            }
        };
        // A solver that died after answering is dropped like one that
        // answered `unknown`: the answer it gave stands.
        if status == Status::Unknown || self.solver()?.send("(pop 1)").is_err() {
            *self.slot = None;
        }
        Ok(status)
    }

    /// Closes the function's scope. A solver dropped after an `unknown`
    /// has none left to close, and one that died after its last answer
    /// is dropped here: the next function starts a fresh one.
    fn close(self) {
        if let Some(solver) = self.slot.as_mut() {
            if solver.send("(pop 1)").is_err() {
                *self.slot = None;
            }
        }
    }
}

fn ask_once(
    solver: &mut smt::Solver,
    query: &str,
    exec: Option<&exec::Exec<'_>>,
) -> std::result::Result<Status, String> {
    solver.send(query)?;
    Ok(match solver.check()? {
        smt::SatResult::Unsat => Status::Proved,
        smt::SatResult::Unknown => Status::Unknown,
        smt::SatResult::Sat => {
            let counterexample = match exec {
                Some(exec) => {
                    let terms: Vec<String> =
                        exec.inputs.iter().map(|(_, term)| term.clone()).collect();
                    solver
                        .values(&terms)
                        .map(|values| {
                            exec.inputs
                                .iter()
                                .zip(values)
                                .map(|((name, _), value)| (name.clone(), smt::render_int(&value)))
                                .collect()
                        })
                        .unwrap_or_default()
                }
                None => Vec::new(),
            };
            Status::Failed { counterexample }
        }
    })
}

/// Checks each obligation of `exec` against its declarations and facts.
fn solve(
    mut solver: Lane<'_>,
    mut rough: Option<Lane<'_>>,
    mut sample: Option<Lane<'_>>,
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
    // The weaker problem has each float `/` and `sqrt` result's facts in
    // place of the fact that defines it.
    let mut replaced: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    // Candidate inputs until a check needs samples, then the allowed ones.
    let mut candidates = Vec::new();
    let mut samples: Option<Vec<String>> = None;
    if let Some(rough) = rough.as_mut() {
        rough.send(&prelude)?;
        let all_literals =
            float::all_literals(
                exec.facts
                    .iter()
                    .map(String::as_str)
                    .chain(exec.obligations.iter().filter_map(
                        |obligation| match &obligation.goal {
                            exec::Goal::Holds(goal) => Some(goal.as_str()),
                            exec::Goal::Unsupported(_) => None,
                        },
                    )),
            );
        let literals = float::literals(&all_literals);
        for rounded in &exec.rounded {
            replaced.insert(rounded.fact, float::axioms(rounded, &literals));
        }
        candidates = sample_preludes(exec, &all_literals);
    }
    // Samples are cheap, not free: a function gets this many attempts.
    let mut attempts = SAMPLE_CHECKS * float::SAMPLES;
    let mut statuses = Vec::new();
    let mut asserted = 0usize;
    for obligation in &exec.obligations {
        // Facts are asserted in the order the walk assumed them, so each
        // check sees only what holds before it.
        if obligation.facts > asserted {
            let mut text = String::new();
            let mut rough_text = String::new();
            for (index, fact) in exec.facts[asserted..obligation.facts].iter().enumerate() {
                text.push_str(&format!("(assert {fact})\n"));
                match replaced.get(&(asserted + index)) {
                    Some(axioms) => {
                        for axiom in axioms {
                            rough_text.push_str(&format!("(assert {axiom})\n"));
                        }
                    }
                    None => rough_text.push_str(&format!("(assert {fact})\n")),
                }
            }
            if trace {
                eprintln!("{text}");
            }
            solver.send(&text)?;
            if let Some(rough) = rough.as_mut() {
                rough.send(&rough_text)?;
            }
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
        // A check the weaker problem proves is proved.
        if let Some(rough) = rough.as_mut() {
            if rough.ask(&query, None)? == Status::Proved {
                if trace {
                    eprintln!("; => Proved from the facts of float `/` and `sqrt`");
                }
                statuses.push(Status::Proved);
                continue;
            }
        }
        // A check that fails with its float inputs fixed fails.
        let mut refuted = None;
        if let Some(lane) = sample.as_mut() {
            if samples.is_none() {
                let drawn = std::mem::take(&mut candidates);
                samples = Some(allowed_samples(lane, exec, drawn, trace)?.unwrap_or_default());
            }
            let rows = samples.as_mut().expect("the samples were just drawn");
            let mut facts = String::from("; the check with its float inputs fixed\n");
            for fact in &exec.facts[..obligation.facts] {
                facts.push_str(&format!("(assert {fact})\n"));
            }
            let goal = &query["(push 1)\n".len()..];
            for fixed in rows.iter().take(attempts) {
                attempts -= 1;
                match lane.ask(&format!("(push 1)\n{fixed}{facts}{goal}"), Some(exec))? {
                    Status::Proved => {}
                    status @ Status::Failed { .. } => {
                        refuted = Some(status);
                        break;
                    }
                    // The solver could not compute this one in time, so
                    // the rest of the function goes without samples.
                    _ => {
                        attempts = 0;
                        break;
                    }
                }
            }
        }
        if let Some(status) = refuted {
            if trace {
                eprintln!("; => {status:?} with the float inputs fixed");
            }
            statuses.push(status);
            continue;
        }
        let status = solver.ask(&query, Some(exec))?;
        if trace {
            eprintln!("; => {status:?}");
        }
        statuses.push(status);
    }
    solver.close();
    if let Some(rough) = rough {
        rough.close();
    }
    Ok(statuses)
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;

/// The builtin enum a checked list read returns: `Found(element)` or
/// `Missing`.
pub(crate) const LOOKUP: &str = "Lookup";
