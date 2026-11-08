//! Contract clauses (verification checkpoint, stage V1): `requires` and
//! `ensures` on functions, `invariant` and `decreases` on loops.
//!
//! A clause's condition is a specification expression: it must have type
//! `bool` and must not change program state. It may read parameters, `self`,
//! module constants, fields, and elements, and call the pure builtins and
//! read-only builtin methods listed below. Inside `ensures`, `result` names
//! the value the function returns.
//!
//! The checker types each condition against a copy of the entry scope, so a
//! clause never changes the ownership state the body starts from.

use super::{
    BTreeMap, Diagnostic, Expr, ExprKind, FunctionChecker, FunctionDecl, HashMap, LocalBinding,
    Result, Type,
};
use crate::ast::{
    Argument, AssignTarget, ComprehensionOutput, ContractClause, ContractKind, FormatPart,
    OldValue, Stmt,
};
use aura_core::ops::ReceiverKind;

const CODE: &str = "AU2048";

/// The name the return value takes inside an `ensures` condition.
pub(crate) const RESULT_NAME: &str = "result";

/// Builtin functions a condition may call. Each reads its arguments only.
const PURE_FUNCTIONS: &[&str] = &[
    "len",
    "abs",
    "min",
    "max",
    "sum",
    "any",
    "all",
    "ord",
    "chr",
    "str",
    "round",
    "sqrt",
    "sorted",
    "reversed",
    "list",
    "divmod",
    "range",
    "enumerate",
    "zip",
];

/// Builtin methods a condition may call. Each reads its receiver and
/// arguments only.
const PURE_METHODS: &[&str] = &[
    "len",
    "byte_len",
    "contains",
    "starts_with",
    "ends_with",
    "find",
    "count",
    "is_empty",
    "keys",
    "values",
    "items",
    "get",
    "index",
    "to_lower",
    "to_upper",
    "trim",
    "trim_start",
    "trim_end",
    "split",
    "lines",
    "chars",
    "split_whitespace",
    "to_string",
    "to_float",
    "copy",
    "clone",
    "shape",
    "mean",
];

impl FunctionChecker<'_> {
    /// Checks every clause of `function` against its entry scope.
    pub(super) fn check_contracts(
        &self,
        function: &FunctionDecl,
        locals: &HashMap<String, LocalBinding>,
        return_type: &Type,
    ) -> Result<()> {
        *self.recursion_measure.borrow_mut() = None;
        for clause in &function.contracts {
            if clause.kind == ContractKind::Ensures {
                if function.view_return.is_some() {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "a function with a view result cannot declare `ensures` yet",
                    ));
                }
                if function.name == super::INITIALIZER_NAME {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "`__init__` cannot declare `ensures` yet; state the promise as a class invariant later, or check it in a factory",
                    ));
                }
                if function
                    .params
                    .iter()
                    .any(|param| param.name == RESULT_NAME)
                {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "`result` names the return value in an `ensures` clause; rename the parameter `result`",
                    ));
                }
                if let Some(param) = function.params.iter().find(|param| {
                    locals.get(&param.name).is_some_and(|binding| {
                        binding.passing == ReceiverKind::Value && !self.is_copy_type(&binding.ty)
                    }) && super::expr_references_name(&clause.condition, &param.name)
                }) {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        format!(
                            "an `ensures` clause cannot read `own` parameter `{}`, which the body may move",
                            param.name
                        ),
                    ));
                }
            }
            if clause.kind == ContractKind::Decreases {
                self.check_function_measure(function, clause)?;
            }
            let mut scope = locals.clone();
            if clause.kind == ContractKind::Ensures {
                scope.insert(RESULT_NAME.to_string(), result_binding(return_type.clone()));
                for old in &clause.olds {
                    let ty = self.check_old_value(function, old, locals)?;
                    scope.insert(old.name.clone(), result_binding(ty));
                }
            }
            self.check_clause_condition(clause, &mut scope)?;
        }
        Ok(())
    }

    /// Checks a function's `decreases` measure and records, for each
    /// parameter in order, the name the measure reads it by.
    fn check_function_measure(
        &self,
        function: &FunctionDecl,
        clause: &ContractClause,
    ) -> Result<()> {
        let mut measure = self.recursion_measure.borrow_mut();
        if measure.is_some() {
            return Err(Diagnostic::coded_at(
                CODE,
                clause.span,
                "a function takes at most one `decreases` clause",
            ));
        }
        let params = function
            .params
            .iter()
            .map(|param| {
                super::expr_references_name(&clause.condition, &param.name)
                    .then(|| param.name.clone())
            })
            .collect();
        *measure = Some((function.name.clone(), params));
        Ok(())
    }

    /// A call that a function with a `decreases` measure makes to a
    /// function or method that declares one too passes a pure argument for
    /// each parameter the callee's measure reads, and a receiver that names
    /// a place when that measure reads `self`, so the lowering can evaluate
    /// the callee's measure before the call when the two recurse together.
    pub(super) fn check_recursive_call(
        &self,
        callee: &Expr,
        args: &[Argument],
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if self.recursion_measure.borrow().is_none() {
            return Ok(());
        }
        let (decl, receiver) = match &callee.kind {
            ExprKind::Name(name) if !locals.contains_key(name) => match self.functions.get(name) {
                Some(function) => (&function.decl, None),
                None => return Ok(()),
            },
            ExprKind::Member { object, field } => {
                let Ok(Type::Named(class, _)) =
                    self.with_spec(|| self.type_of_expr(object, &mut locals.clone()))
                else {
                    return Ok(());
                };
                match self
                    .classes
                    .get(&class)
                    .and_then(|class| class.methods.get(field))
                {
                    Some(method) if method.decl.receiver.is_some() => {
                        (&method.decl, Some(object.as_ref()))
                    }
                    _ => return Ok(()),
                }
            }
            _ => return Ok(()),
        };
        let Some(clause) = decl
            .contracts
            .iter()
            .find(|clause| clause.kind == ContractKind::Decreases)
        else {
            return Ok(());
        };
        if let Some(receiver) = receiver {
            if super::expr_references_name(&clause.condition, "self") && !is_place(receiver) {
                return Err(Diagnostic::coded_at(
                    CODE,
                    receiver.span,
                    "the receiver of a call whose `decreases` measure reads `self` must name a variable or a field of one",
                ));
            }
        }
        let params: Vec<Option<&str>> = decl
            .params
            .iter()
            .map(|param| {
                super::expr_references_name(&clause.condition, &param.name)
                    .then_some(param.name.as_str())
            })
            .collect();
        for (index, arg) in args.iter().enumerate() {
            let measured = match &arg.name {
                Some(name) => params.iter().flatten().any(|param| param == name),
                None => params.get(index).is_some_and(Option::is_some),
            };
            if measured {
                let mut receivers = Vec::new();
                self.reject_impure(
                    &arg.value,
                    "an argument the `decreases` measure reads",
                    &mut receivers,
                )?;
            }
        }
        Ok(())
    }

    /// Checks one `old(expr)`: a pure read of a parameter or `self`, or of
    /// a field or element of one, whose value can be copied on entry.
    fn check_old_value(
        &self,
        function: &FunctionDecl,
        old: &OldValue,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let reads_parameter = old_root(&old.expr).is_some_and(|root| {
            root == "self" || function.params.iter().any(|param| param.name == root)
        });
        if !reads_parameter {
            return Err(Diagnostic::coded_at(
                CODE,
                old.expr.span,
                "`old` reads a parameter or `self`, or a field or element of one",
            ));
        }
        let mut receivers = Vec::new();
        self.reject_impure(&old.expr, "an `old` value", &mut receivers)?;
        self.check_receivers(receivers, &mut locals.clone(), "an `old` value")?;
        let ty = self.with_spec(|| self.type_of_expr(&old.expr, &mut locals.clone()))?;
        let copyable = self.is_copy_type(&ty)
            || matches!(&ty, Type::Named(name, _) if matches!(name.as_str(), "str" | "list" | "dict" | "set" | "Array"));
        if !copyable {
            return Err(Diagnostic::coded_at(
                CODE,
                old.expr.span,
                format!(
                    "`old` keeps a copy of its value, so it takes a Copy type, `str`, or a list, dict, set, or Array, not `{ty}`"
                ),
            ));
        }
        if !self.is_copy_type(&ty) {
            self.reject_rng_duplication("`old`", &ty, old.expr.span)?;
        }
        Ok(ty)
    }

    /// Checks a `spec def`: a module function whose body is one `return`
    /// of a pure expression, with no `mut` parameters and no clauses.
    pub(super) fn check_spec_function(
        &self,
        function: &FunctionDecl,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
    ) -> Result<()> {
        if let Some(param) = function.params.iter().find(|param| {
            locals
                .get(&param.name)
                .is_some_and(|binding| binding.passing == ReceiverKind::BorrowMut)
        }) {
            return Err(Diagnostic::coded_at(
                CODE,
                param.span,
                format!("a `spec def` cannot take `mut` parameter `{}`", param.name),
            ));
        }
        if function.receiver.is_some() && function.receiver != Some(ReceiverKind::Borrow) {
            return Err(Diagnostic::coded_at(
                CODE,
                function.span,
                "a `spec def` method reads `self`; it cannot take `mut self` or `owned self`",
            ));
        }
        // A `spec def` may state when it is defined.
        if let Some(clause) = function
            .contracts
            .iter()
            .find(|clause| clause.kind != ContractKind::Requires)
        {
            return Err(Diagnostic::coded_at(
                CODE,
                clause.span,
                format!(
                    "a `spec def` states only `requires`; its body is its result, so it takes no `{}`",
                    clause.kind.keyword()
                ),
            ));
        }
        for clause in &function.contracts {
            self.check_clause_condition(clause, &mut locals.clone())?;
        }
        let [crate::ast::Stmt::Return(crate::ast::ReturnStmt {
            value: Some(value),
            view: None,
            ..
        })] = function.body.as_slice()
        else {
            return Err(Diagnostic::coded_at(
                CODE,
                function.span,
                "a `spec def` body is one `return` of a pure expression, after an optional docstring",
            ));
        };
        let mut receivers = Vec::new();
        self.reject_impure(value, "a `spec def` body", &mut receivers)?;
        self.check_receivers(receivers, &mut locals.clone(), "a `spec def` body")?;
        self.with_spec(|| {
            self.check_block(&function.body, locals, return_type, 0, true)
                .map(|_| ())
        })
    }

    /// Checks a `lemma def`: a proof with no result and no `mut`
    /// parameters, whose body only states facts. It may read `spec def`
    /// helpers, since it never runs where a call is erased.
    pub(super) fn check_lemma(
        &self,
        function: &FunctionDecl,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
    ) -> Result<()> {
        if *return_type != Type::Unit {
            return Err(Diagnostic::coded_at(
                CODE,
                function.span,
                "a `lemma def` returns nothing; it states its facts as `ensures` clauses",
            ));
        }
        if let Some(param) = function.params.iter().find(|param| {
            locals
                .get(&param.name)
                .is_some_and(|binding| binding.passing == ReceiverKind::BorrowMut)
        }) {
            return Err(Diagnostic::coded_at(
                CODE,
                param.span,
                format!("a `lemma def` cannot take `mut` parameter `{}`", param.name),
            ));
        }
        self.check_lemma_body(&function.body)?;
        self.check_contracts(function, locals, return_type)?;
        self.with_spec(|| {
            self.check_block(&function.body, locals, return_type, 0, true)
                .map(|_| ())
        })
    }

    /// A lemma body holds only what states or proves a fact: `assert`,
    /// `if`, loops, local assignments of pure values, and calls of other
    /// lemmas, each pure.
    fn check_lemma_body(&self, body: &[Stmt]) -> Result<()> {
        let pure = |expr: &Expr| self.reject_impure(expr, "a `lemma def` body", &mut Vec::new());
        for stmt in body {
            match stmt {
                Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
                Stmt::Return(ret) if ret.value.is_none() => {}
                Stmt::Assert(assert) => {
                    pure(&assert.condition)?;
                    if let Some(message) = &assert.message {
                        pure(message)?;
                    }
                }
                Stmt::Assign(assign) if matches!(assign.target, AssignTarget::Name(_)) => {
                    pure(&assign.value)?;
                }
                Stmt::If(if_stmt) => {
                    for branch in &if_stmt.branches {
                        pure(&branch.condition)?;
                        self.check_lemma_body(&branch.body)?;
                    }
                    if let Some(body) = &if_stmt.else_body {
                        self.check_lemma_body(body)?;
                    }
                }
                Stmt::While(while_stmt) => {
                    pure(&while_stmt.condition)?;
                    self.check_lemma_body(&while_stmt.body)?;
                }
                Stmt::For(for_stmt) if for_stmt.borrow_mode.is_none() => {
                    pure(&for_stmt.iterable)?;
                    self.check_lemma_body(&for_stmt.body)?;
                }
                Stmt::Expr(expr_stmt) if self.lemma_call(&expr_stmt.expr).is_some() => {}
                _ => {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        super::statement_span(stmt),
                        "a `lemma def` body holds only `assert`, `if`, loops, assignments of local names, and lemma calls, each pure",
                    ));
                }
            }
        }
        Ok(())
    }

    /// The arguments of `expr`, when it is a call of a `lemma def`.
    fn lemma_call<'e>(&self, expr: &'e Expr) -> Option<&'e [Argument]> {
        let ExprKind::Call { callee, args } = &expr.kind else {
            return None;
        };
        let ExprKind::Name(name) = &callee.kind else {
            return None;
        };
        self.functions
            .get(name)
            .is_some_and(|function| function.decl.lemma)
            .then_some(args.as_slice())
    }

    /// A lemma call written as a statement is erased at run time, so its
    /// arguments must be pure: nothing they do may be lost.
    pub(super) fn check_lemma_call_arguments(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let Some(args) = self.lemma_call(expr) else {
            return Ok(());
        };
        // A local of the same name is called instead.
        if matches!(&expr.kind, ExprKind::Call { callee, .. }
            if matches!(&callee.kind, ExprKind::Name(name) if locals.contains_key(name)))
        {
            return Ok(());
        }
        for arg in args {
            self.reject_impure(&arg.value, "an argument of a lemma call", &mut Vec::new())?;
        }
        Ok(())
    }

    /// A `ghost` local's value, or an `assert` that reads one, is erased
    /// when the program runs, so it must be pure: nothing it does may be
    /// lost.
    pub(super) fn check_ghost_use(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        what: &str,
    ) -> Result<()> {
        let mut receivers = Vec::new();
        self.reject_impure(expr, what, &mut receivers)?;
        self.check_receivers(receivers, &mut locals.clone(), what)
    }

    /// Runs `check` with `spec def` references allowed.
    fn with_spec<T>(&self, check: impl FnOnce() -> Result<T>) -> Result<T> {
        let outer = self.in_spec.replace(true);
        let result = check();
        self.in_spec.set(outer);
        result
    }

    /// Rejects naming a `spec def` outside a contract clause or another
    /// `spec def` body.
    pub(super) fn reject_spec_reference(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if self.in_spec.get() {
            return Ok(());
        }
        let name = match &expr.kind {
            ExprKind::Name(name) => name,
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Name(name) => name,
                _ => return Ok(()),
            },
            _ => return Ok(()),
        };
        if locals.contains_key(name) || !self.is_spec_function(name) {
            return Ok(());
        }
        Err(Diagnostic::coded_at(
            CODE,
            expr.span,
            format!(
                "`{name}` is a `spec def`; only contract clauses and other `spec def` bodies may use it"
            ),
        ))
    }

    fn is_spec_function(&self, name: &str) -> bool {
        self.resolve_function_info(name)
            .is_some_and(|function| function.decl.spec)
    }

    /// [`reject_impure_condition`] with this module's `spec def` functions
    /// allowed.
    fn reject_impure<'e>(
        &self,
        expr: &'e Expr,
        what: &str,
        receivers: &mut Vec<(&'e Expr, &'e str)>,
    ) -> Result<()> {
        reject_impure_condition(
            expr,
            what,
            receivers,
            &|name| match name.strip_prefix('.') {
                Some(method) => self.is_spec_method_name(method),
                None => self.is_spec_function(name),
            },
        )
    }

    /// Whether some class of this module has a `spec def` method `name`.
    fn is_spec_method_name(&self, name: &str) -> bool {
        self.classes.values().any(|class| {
            class
                .methods
                .get(name)
                .is_some_and(|method| method.decl.spec)
        })
    }

    /// Checks that each method a pure condition calls by name is a read-only
    /// builtin method, or a `spec def` method of the receiver's class.
    fn check_receivers(
        &self,
        receivers: Vec<(&Expr, &str)>,
        scope: &mut HashMap<String, LocalBinding>,
        what: &str,
    ) -> Result<()> {
        for (receiver, method) in receivers {
            let ty = self.with_spec(|| self.type_of_expr(receiver, scope))?;
            let spec_method = matches!(&ty, Type::Named(class, _) if self
                .classes
                .get(class)
                .and_then(|class| class.methods.get(method))
                .is_some_and(|method| method.decl.spec));
            if spec_method {
                continue;
            }
            if !has_builtin_methods(&ty) || !PURE_METHODS.contains(&method) {
                return Err(impure(
                    receiver,
                    what,
                    &format!("a method call on `{ty}`, which may change program state,"),
                ));
            }
        }
        Ok(())
    }

    /// Checks the clauses of `kinds` at the top of a loop body against the
    /// scope they are evaluated in.
    pub(super) fn check_loop_clauses(
        &self,
        clauses: &[ContractClause],
        kinds: &[ContractKind],
        locals: &HashMap<String, LocalBinding>,
        for_loop: bool,
    ) -> Result<()> {
        let mut measures = 0;
        for clause in clauses.iter().filter(|clause| kinds.contains(&clause.kind)) {
            if clause.kind == ContractKind::Decreases {
                if for_loop {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "a `for` loop always ends, so it takes no `decreases` clause; `decreases` belongs on `while`",
                    ));
                }
                measures += 1;
                if measures > 1 {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        clause.span,
                        "a loop takes at most one `decreases` clause",
                    ));
                }
            }
            self.check_clause_condition(clause, &mut locals.clone())?;
        }
        Ok(())
    }

    /// Checks one clause's condition: pure, and `bool` (or `int64` for a
    /// `decreases` measure).
    fn check_clause_condition(
        &self,
        clause: &ContractClause,
        scope: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let mut receivers = Vec::new();
        let what = format!(
            "{} `{}` condition",
            if clause.kind == ContractKind::Invariant {
                "an"
            } else {
                "a"
            },
            clause.kind.keyword()
        );
        self.reject_impure(&clause.condition, &what, &mut receivers)?;
        let (expected, noun) = match clause.kind {
            ContractKind::Decreases => (Type::named("int64"), "measure"),
            _ => (Type::named("bool"), "condition"),
        };
        let actual =
            self.with_spec(|| self.type_of_expr_hint(&clause.condition, scope, Some(&expected)))?;
        if actual != expected {
            return Err(Diagnostic::coded_at(
                "AU2002",
                clause.condition.span,
                format!(
                    "{} `{}` {noun} must be `{expected}`, found `{actual}`",
                    if clause.kind == ContractKind::Invariant {
                        "an"
                    } else {
                        "a"
                    },
                    clause.kind.keyword()
                ),
            ));
        }
        // A method allowed by name must also be a builtin's, or a `spec def`
        // of the receiver's class: a user type's method of the same name may
        // change state.
        self.check_receivers(receivers, scope, &what)
    }
}

/// True when `ty`'s methods are the builtin read-only ones of
/// [`PURE_METHODS`].
fn has_builtin_methods(ty: &Type) -> bool {
    match ty {
        Type::Named(name, _) => matches!(
            name.as_str(),
            "list" | "dict" | "set" | "str" | "bytes" | "Array" | "Span" | "Range"
        ),
        Type::Tuple(_) => true,
        _ => false,
    }
}

/// The read-only binding `result` has inside an `ensures` condition.
fn result_binding(ty: Type) -> LocalBinding {
    LocalBinding {
        ty,
        assignable: false,
        mutable_place: false,
        managed_resource: false,
        passing: ReceiverKind::Borrow,
        borrow_origin: None,
        borrowed_at: None,
        match_borrow_place: None,
        stale_match_borrow_place: None,
        shared_match_scrutinee: None,
        moved: false,
        moved_at: None,
        moved_fields: BTreeMap::new(),
        frozen_places: BTreeMap::new(),
        shared_match_places: BTreeMap::new(),
        captured: false,
        view: None,
        closure_loans: Vec::new(),
        narrowed: BTreeMap::new(),
        stale_narrowing: BTreeMap::new(),
    }
}

fn impure(expr: &Expr, context: &str, what: &str) -> Diagnostic {
    Diagnostic::coded_at(
        CODE,
        expr.span,
        format!("{context} must not change program state; {what} is not allowed here"),
    )
    .with_help("conditions may read values and call pure builtins such as `len`, `min`, `all`, read-only methods such as `contains`, and `spec def` functions")
}

/// The parameter or `self` an `old` path starts from.
fn old_root(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Name(name) => Some(name),
        ExprKind::Member { object, .. }
        | ExprKind::Index { object, .. }
        | ExprKind::Slice { object, .. } => old_root(object),
        ExprKind::Group(inner) => old_root(inner),
        // A method call reads its receiver: `old(self.headroom())`.
        ExprKind::Call { callee, .. } => match &callee.kind {
            ExprKind::Member { object, .. } => old_root(object),
            _ => None,
        },
        _ => None,
    }
}

/// Rejects a condition that could change program state.
///
/// Method calls are allowed by name here; `receivers` collects each call's
/// receiver so the caller can check that it has a builtin type.
fn reject_impure_condition<'e>(
    expr: &'e Expr,
    kind: &str,
    receivers: &mut Vec<(&'e Expr, &'e str)>,
    is_spec: &dyn Fn(&str) -> bool,
) -> Result<()> {
    macro_rules! check {
        ($expr:expr) => {
            reject_impure_condition($expr, kind, receivers, is_spec)
        };
    }
    match &expr.kind {
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_) => Ok(()),
        ExprKind::FString(parts) => {
            for part in parts {
                match part {
                    FormatPart::Literal(_) => {}
                    FormatPart::Expr(expr) | FormatPart::Formatted { expr, .. } => check!(expr)?,
                }
            }
            Ok(())
        }
        ExprKind::Tuple(items) | ExprKind::List(items) | ExprKind::Set(items) => {
            for item in items {
                check!(item)?;
            }
            Ok(())
        }
        ExprKind::Map(entries) => {
            for entry in entries {
                check!(&entry.key)?;
                check!(&entry.value)?;
            }
            Ok(())
        }
        ExprKind::Comprehension { output, clauses } => {
            match output {
                ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                    check!(value)?
                }
                ComprehensionOutput::Map { key, value } => {
                    check!(key)?;
                    check!(value)?;
                }
            }
            for clause in clauses {
                check!(&clause.iterable)?;
                for filter in &clause.filters {
                    check!(filter)?;
                }
            }
            Ok(())
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } | ExprKind::Group(expr) => {
            check!(expr)
        }
        ExprKind::Binary { left, right, .. } => {
            check!(left)?;
            check!(right)
        }
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => {
            check!(then_expr)?;
            check!(condition)?;
            check!(else_expr)
        }
        ExprKind::Member { object, .. } => check!(object),
        ExprKind::Index { object, index } => {
            check!(object)?;
            check!(index)
        }
        ExprKind::Slice {
            object, start, end, ..
        } => {
            check!(object)?;
            for bound in start.iter().chain(end.iter()) {
                check!(bound)?;
            }
            Ok(())
        }
        ExprKind::Membership {
            value, container, ..
        } => {
            check!(value)?;
            check!(container)
        }
        ExprKind::CompareChain { first, links } => {
            check!(first)?;
            for link in links {
                check!(&link.operand)?;
            }
            Ok(())
        }
        ExprKind::IsNone { value, .. } => check!(value),
        ExprKind::Call { callee, args } => {
            let allowed = match &callee.kind {
                ExprKind::Name(name) if name == "old" => {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        expr.span,
                        "`old(...)` names a value on entry; it is allowed only in an `ensures` condition, around a parameter or a field or element of one",
                    ));
                }
                ExprKind::Name(name) if name == "forall" || name == "exists" => {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        expr.span,
                        format!("write `{name}(name in collection, condition)`"),
                    ));
                }
                ExprKind::Name(name) => PURE_FUNCTIONS.contains(&name.as_str()) || is_spec(name),
                ExprKind::Member { object, field } => {
                    check!(object)?;
                    receivers.push((object, field.as_str()));
                    PURE_METHODS.contains(&field.as_str()) || is_spec(&format!(".{field}"))
                }
                _ => false,
            };
            if !allowed {
                return Err(impure(
                    expr,
                    kind,
                    "a call to a user function or a method that may change its receiver",
                ));
            }
            for arg in args {
                check!(&arg.value)?;
            }
            Ok(())
        }
        ExprKind::Lambda { .. } => Err(impure(expr, kind, "a lambda")),
        ExprKind::Specialize { .. } => Err(impure(expr, kind, "an explicit specialization")),
        ExprKind::Try(_) => Err(impure(expr, kind, "`try`, which can return early,")),
        ExprKind::Match { .. } => Err(impure(expr, kind, "a `match` expression")),
    }
}

/// Whether `expr` names a variable or a field of one, through any grouping.
fn is_place(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Name(_) => true,
        ExprKind::Member { object, .. } => is_place(object),
        ExprKind::Group(inner) => is_place(inner),
        _ => false,
    }
}
