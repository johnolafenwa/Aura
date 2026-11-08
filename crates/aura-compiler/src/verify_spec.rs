//! Contract clauses as SMT terms, for the calls that rely on them.
//!
//! A caller proves its callee's `requires` and assumes its `ensures`. The
//! clauses are read from the callee's declaration, with each parameter
//! bound to the symbolic value the call passes, so a call never depends on
//! the callee's body (modular verification). A clause must also be
//! well-defined: an index in range, a divisor that is not zero, and
//! arithmetic inside its type, since the runtime check would stop the
//! program otherwise. A clause outside the modeled subset is an
//! unsupported obligation at the call, and is not assumed after it.

use std::collections::BTreeMap;

use super::exec::{
    conjoin, key_sort, not, scalar_val, smt_string, structural_eq, Exec, FloatTy, Goal, IntTy,
    State, Val,
};
use super::ObligationKind;
use crate::ast::{
    BinaryOp, BindingTarget, CompareOp, ComprehensionOutput, ContractClause, ContractKind, Expr,
    ExprKind, FunctionDecl, Stmt, UnaryOp,
};
use crate::diag::Span;

/// How deep `spec def` calls may unfold inside one clause.
const MAX_SPEC_DEPTH: usize = 8;

type Env = BTreeMap<String, Val>;

impl Exec<'_> {
    /// Proves each `requires` of `callee`, and the class invariant of a
    /// receiver that a public `mut self` method or an invariant check reads.
    pub(super) fn call_requires(
        &mut self,
        state: &State,
        callee: &str,
        pre: &[(String, Val)],
        span: Span,
    ) {
        let Some(decl) = self.contract_override.or_else(|| self.cx.decl(callee)) else {
            return;
        };
        let env: Env = pre.iter().cloned().collect();
        let display = public_name(callee);
        let checks_invariant = is_invariant_check(callee);
        for clause in &decl.contracts {
            let kind = match clause.kind {
                ContractKind::Requires => ObligationKind::Requires,
                ContractKind::Invariant if checks_invariant => ObligationKind::ClassInvariant,
                _ => continue,
            };
            let message = match kind {
                ObligationKind::ClassInvariant => {
                    format!("class invariant holds: `{}`", clause.text)
                }
                _ => format!("`requires` of `{display}` holds: `{}`", clause.text),
            };
            self.prove_clause(state, kind, span, message, clause, &env);
        }
        if let Some(clauses) = self.guarded_invariant(callee) {
            if let Some((_, receiver)) = pre.first() {
                let env: Env = [("self".to_string(), receiver.clone())].into();
                // No run checks the invariant on entry to a method that
                // only reads `self`; the method's proof assumes it.
                let reads_only = self.reads_only(callee);
                let erasing = self.erasing;
                self.erasing |= reads_only;
                for clause in clauses {
                    let message = if reads_only {
                        format!(
                            "class invariant holds before `{display}`, which assumes it: `{}`",
                            clause.text
                        )
                    } else {
                        format!(
                            "class invariant holds before `{display}`: `{}`",
                            clause.text
                        )
                    };
                    self.prove_clause(
                        state,
                        ObligationKind::ClassInvariant,
                        span,
                        message,
                        &clause,
                        &env,
                    );
                }
                self.erasing = erasing;
            }
        }
        // An object of a class with invariants passed to a function holds
        // them: the function's proof assumes they do.
        for (param, value) in pre.iter().filter(|(param, _)| param != "self") {
            let Some(clauses) = self.record_invariant(value) else {
                continue;
            };
            let erasing = self.erasing;
            self.erasing = true;
            let env: Env = [("self".to_string(), value.clone())].into();
            for clause in clauses {
                let message = format!(
                    "class invariant holds for `{param}` passed to `{display}`: `{}`",
                    clause.text
                );
                self.prove_clause(
                    state,
                    ObligationKind::ClassInvariant,
                    span,
                    message,
                    &clause,
                    &env,
                );
            }
            self.erasing = erasing;
        }
    }

    /// The invariant clauses of the class of `value`, when it is an object
    /// of a class with invariants.
    fn record_invariant(&self, value: &Val) -> Option<Vec<ContractClause>> {
        let Val::Record(class, _) = value else {
            return None;
        };
        let clauses = self.cx.class_invariant(class)?;
        (!clauses.is_empty()).then(|| clauses.to_vec())
    }

    /// Assumes the invariants of each object of a class with invariants
    /// among `values`.
    fn assume_record_invariants<'v>(
        &mut self,
        state: &State,
        values: impl IntoIterator<Item = &'v Val>,
    ) {
        for value in values {
            if let Some(clauses) = self.record_invariant(value) {
                let env: Env = [("self".to_string(), value.clone())].into();
                for clause in clauses {
                    self.assume_clause(state, &clause, &env);
                }
            }
        }
    }

    /// At a return, the objects of classes with invariants that leave the
    /// function, its result and its `mut` parameters, hold them: callers
    /// assume they do.
    pub(super) fn prove_return_invariants(&mut self, state: &State, result: &Val) {
        let mut values = vec![("the result".to_string(), result.clone())];
        for param in &self.function.params {
            if param.passing == crate::mir::MirReceiverKind::BorrowMut {
                if let Some(value) = state.env.get(&param.name) {
                    values.push((format!("`{}`", param.name), value.clone()));
                }
            }
        }
        let erasing = self.erasing;
        self.erasing = true;
        for (label, value) in values {
            let Some(clauses) = self.record_invariant(&value) else {
                continue;
            };
            let env: Env = [("self".to_string(), value)].into();
            for clause in clauses {
                let message = format!(
                    "class invariant holds for {label} on return: `{}`",
                    clause.text
                );
                self.prove_clause(
                    state,
                    ObligationKind::ClassInvariant,
                    self.span,
                    message,
                    &clause,
                    &env,
                );
            }
        }
        self.erasing = erasing;
    }

    /// Whether `callee` is a method that only reads `self`.
    fn reads_only(&self, callee: &str) -> bool {
        self.cx
            .decl(callee)
            .is_some_and(|decl| decl.receiver == Some(crate::ast::ReceiverKind::Borrow))
    }

    /// Assumes each `ensures` of `callee`, with `result` and the values the
    /// call may have changed.
    pub(super) fn call_ensures(
        &mut self,
        state: &State,
        callee: &str,
        pre: &[(String, Val)],
        post: &[(String, Val)],
        result: &Val,
    ) {
        let Some(decl) = self.contract_override.or_else(|| self.cx.decl(callee)) else {
            return;
        };
        let pre_env: Env = pre.iter().cloned().collect();
        let checks_invariant = is_invariant_check(callee);
        for clause in &decl.contracts {
            match clause.kind {
                ContractKind::Ensures => {
                    let mut env: Env = post.iter().cloned().collect();
                    env.insert("result".to_string(), result.clone());
                    let mut olds_ok = true;
                    for old in &clause.olds {
                        let mut wd = Vec::new();
                        match self.spec(&old.expr, &pre_env, &mut wd, 0) {
                            Ok(value) => {
                                env.insert(old.name.clone(), value);
                            }
                            Err(_) => olds_ok = false,
                        }
                    }
                    if olds_ok {
                        self.assume_clause(state, clause, &env);
                    }
                }
                ContractKind::Invariant if checks_invariant => {
                    self.assume_clause(state, clause, &pre_env);
                }
                _ => {}
            }
        }
        if let Some(clauses) = self.guarded_invariant(callee) {
            if let Some((_, receiver)) = post.first() {
                let env: Env = [("self".to_string(), receiver.clone())].into();
                for clause in clauses {
                    self.assume_clause(state, &clause, &env);
                }
            }
        }
        // A function returns objects of classes with invariants that hold
        // them, through its result and its `mut` parameters.
        let returned: Vec<Val> = std::iter::once(result.clone())
            .chain(
                post.iter()
                    .filter(|(param, _)| param != "self")
                    .map(|(_, value)| value.clone()),
            )
            .collect();
        self.assume_record_invariants(state, &returned);
    }

    /// A named function passed as a callable alias with a contract meets
    /// it: on any arguments the alias's `requires` allows, the function's
    /// `requires` holds, and its `ensures` gives the alias's.
    pub(super) fn prove_callable_refinement(&mut self, state: &State, function: &str, alias: &str) {
        let span = self.span;
        let pc = state.pc.clone();
        let (Some(decl), Some(info)) = (self.cx.decl(function), self.cx.alias_contract(alias))
        else {
            return;
        };
        let display = public_name(function);
        let goals = (|| -> Result<(String, String), String> {
            if decl.params.len() != info.decl.params.len() {
                return Err(format!("`{display}` takes a different number of arguments"));
            }
            let mut alias_env = Env::new();
            let mut function_env = Env::new();
            for ((alias_param, ty), param) in info
                .decl
                .params
                .iter()
                .zip(&info.signature.params)
                .zip(&decl.params)
            {
                let value = self.fresh(ty, &alias_param.name);
                alias_env.insert(alias_param.name.clone(), value.clone());
                function_env.insert(param.name.clone(), value);
            }
            let result = self.fresh(&info.signature.return_type, "result");
            let terms = |clauses: &[ContractClause],
                         kind: ContractKind,
                         env: &Env,
                         exec: &mut Self|
             -> Result<String, String> {
                let mut parts = Vec::new();
                for clause in clauses.iter().filter(|clause| clause.kind == kind) {
                    if !clause.olds.is_empty() {
                        return Err("`old` in a callable contract is not modeled yet".into());
                    }
                    let mut env = env.clone();
                    env.insert("result".to_string(), result.clone());
                    let mut wd = Vec::new();
                    let Val::Bool(term) = exec.spec(&clause.condition, &env, &mut wd, 0)? else {
                        return Err("a clause is not a modeled `bool`".into());
                    };
                    parts.extend(wd);
                    parts.push(term);
                }
                Ok(conjoin(&parts))
            };
            let alias_requires = terms(
                &info.decl.contracts,
                ContractKind::Requires,
                &alias_env,
                self,
            )?;
            let function_requires =
                terms(&decl.contracts, ContractKind::Requires, &function_env, self)?;
            let function_ensures =
                terms(&decl.contracts, ContractKind::Ensures, &function_env, self)?;
            let alias_ensures = terms(
                &info.decl.contracts,
                ContractKind::Ensures,
                &alias_env,
                self,
            )?;
            Ok((
                format!("(=> {alias_requires} {function_requires})"),
                format!("(=> (and {alias_requires} {function_ensures}) {alias_ensures})"),
            ))
        })();
        let (accepts, keeps) = match goals {
            Ok((accepts, keeps)) => (Goal::Holds(accepts), Goal::Holds(keeps)),
            Err(reason) => (Goal::Unsupported(reason.clone()), Goal::Unsupported(reason)),
        };
        self.oblige(
            ObligationKind::Requires,
            span,
            format!("`{display}` accepts every call the contract of `{alias}` allows"),
            &pc,
            accepts,
        );
        self.oblige(
            ObligationKind::Ensures,
            span,
            format!("`{display}` keeps the `ensures` of `{alias}`"),
            &pc,
            keeps,
        );
    }

    /// When an implementation of a trait method states its own `requires`,
    /// a call through the trait proves only the trait's: each of the
    /// implementation's must follow from them.
    pub(super) fn prove_weakened_requires(&mut self, state: &State) {
        let name = self.function.name.clone();
        let (Some(decl), Some(trait_method)) = (self.cx.decl(&name), self.cx.implemented(&name))
        else {
            return;
        };
        let own: Vec<&ContractClause> = decl
            .contracts
            .iter()
            .filter(|clause| clause.kind == ContractKind::Requires)
            .collect();
        let inherited: Vec<&ContractClause> = trait_method
            .contracts
            .iter()
            .filter(|clause| clause.kind == ContractKind::Requires)
            .collect();
        let same = own.len() == inherited.len()
            && own.iter().zip(&inherited).all(|(a, b)| a.text == b.text);
        if same {
            return;
        }
        let env: Env = state.env.clone();
        for clause in inherited {
            self.assume_clause(state, clause, &env);
        }
        let display = public_name(&name);
        for clause in own {
            let message = format!(
                "`requires` of `{display}` follows from the trait method's: `{}`",
                clause.text
            );
            self.prove_clause(
                state,
                ObligationKind::Requires,
                clause.span,
                message,
                clause,
                &env,
            );
        }
    }

    /// On entry, each `requires` that the contract model reads holds and is
    /// well-defined: every call proves it.
    pub(super) fn assume_entry_requires(&mut self, state: &State) {
        let name = self.function.name.clone();
        let Some(decl) = self.cx.decl(&name) else {
            return;
        };
        let env: Env = state.env.clone();
        for clause in &decl.contracts {
            if clause.kind == ContractKind::Requires {
                self.assume_clause(state, clause, &env);
            }
        }
    }

    /// On entry to a public method of a class with invariants that takes
    /// `mut self` or `self`, the invariants hold: every caller proves them.
    pub(super) fn assume_entry_invariant(&mut self, state: &State) {
        let name = self.function.name.clone();
        let Some(clauses) = self.guarded_invariant(&name) else {
            return;
        };
        let Some(receiver) = state.env.get("self").cloned() else {
            return;
        };
        let env: Env = [("self".to_string(), receiver)].into();
        for clause in clauses {
            self.assume_clause(state, &clause, &env);
        }
    }

    /// On entry, each parameter that is an object of a class with
    /// invariants holds them: every call proves it.
    pub(super) fn assume_entry_parameters(&mut self, state: &State) {
        let values: Vec<Val> = self
            .function
            .params
            .iter()
            .filter_map(|param| state.env.get(&param.name).cloned())
            .collect();
        self.assume_record_invariants(state, &values);
    }

    /// The invariant clauses that hold on entry to `callee` and on its
    /// return, when it is a public method of a class with invariants that
    /// takes `mut self` (where they are checked when the program runs) or
    /// `self` (where each call is proved to keep them).
    fn guarded_invariant(&self, callee: &str) -> Option<Vec<ContractClause>> {
        let decl = self.cx.decl(callee)?;
        if !decl.public
            || !matches!(
                decl.receiver,
                Some(crate::ast::ReceiverKind::BorrowMut | crate::ast::ReceiverKind::Borrow)
            )
        {
            return None;
        }
        let (class, _) = base_name(callee).rsplit_once('.')?;
        let clauses = self.cx.class_invariant(class)?;
        (!clauses.is_empty()).then(|| clauses.to_vec())
    }

    fn prove_clause(
        &mut self,
        state: &State,
        kind: ObligationKind,
        span: Span,
        message: String,
        clause: &ContractClause,
        env: &Env,
    ) {
        let mut wd = Vec::new();
        let goal = match self.spec(&clause.condition, env, &mut wd, 0) {
            Ok(Val::Bool(term)) => {
                wd.push(term);
                Goal::Holds(conjoin(&wd))
            }
            Ok(_) => Goal::Unsupported("the clause is not a modeled `bool`".into()),
            Err(reason) => Goal::Unsupported(reason),
        };
        self.oblige(kind, span, message, &state.pc, goal);
    }

    fn assume_clause(&mut self, state: &State, clause: &ContractClause, env: &Env) {
        let mut wd = Vec::new();
        if let Ok(Val::Bool(term)) = self.spec(&clause.condition, env, &mut wd, 0) {
            wd.push(term);
            let fact = conjoin(&wd);
            let pc = state.pc.clone();
            self.assume(&pc, fact);
        }
    }

    /// The value of a specification expression. Each condition the
    /// expression needs to be well-defined is pushed to `wd`.
    pub(super) fn spec(
        &mut self,
        expr: &Expr,
        env: &Env,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<Val, String> {
        match &expr.kind {
            ExprKind::Int(value) => Ok(Val::Int(value.to_string(), IntTy::INT64)),
            ExprKind::Bool(value) => Ok(Val::Bool(value.to_string())),
            ExprKind::Float(value) => Ok(Val::Float(FloatTy::literal(*value), FloatTy::F64)),
            ExprKind::String(text) => smt_string(text)
                .map(Val::Str)
                .ok_or("this string literal is not modeled yet".to_string()),
            ExprKind::Name(name) => env
                .get(name)
                .cloned()
                .ok_or(format!("`{name}` is not modeled in contracts yet")),
            ExprKind::Group(inner) => self.spec(inner, env, wd, depth),
            ExprKind::Unary { op, expr: inner } => {
                let value = self.spec(inner, env, wd, depth)?;
                match (op, value) {
                    (UnaryOp::Not, Val::Bool(term)) => Ok(Val::Bool(not(&term))),
                    (UnaryOp::Neg, Val::Float(term, float)) => {
                        Ok(Val::Float(format!("(fp.neg {term})"), float))
                    }
                    (UnaryOp::Neg, Val::Int(term, ty)) => {
                        let result = format!("(- {term})");
                        wd.push(ty.contains(&result));
                        Ok(Val::Int(result, ty))
                    }
                    _ => Err("this unary operator is not modeled in contracts yet".into()),
                }
            }
            ExprKind::Binary { op, left, right } => {
                self.spec_binary(*op, left, right, env, wd, depth)
            }
            ExprKind::CompareChain { first, links } => {
                let mut previous = self.spec(first, env, wd, depth)?;
                let mut parts = Vec::new();
                for link in links {
                    let next = self.spec(&link.operand, env, wd, depth)?;
                    let op = match link.op {
                        CompareOp::Eq => BinaryOp::Eq,
                        CompareOp::NotEq => BinaryOp::NotEq,
                        CompareOp::Less => BinaryOp::Less,
                        CompareOp::LessEq => BinaryOp::LessEq,
                        CompareOp::Greater => BinaryOp::Greater,
                        CompareOp::GreaterEq => BinaryOp::GreaterEq,
                        CompareOp::In | CompareOp::NotIn => {
                            let held = self.membership(&previous, &next)?;
                            parts.push(if link.op == CompareOp::In {
                                held
                            } else {
                                not(&held)
                            });
                            previous = next;
                            continue;
                        }
                    };
                    parts.push(compare(op, &previous, &next)?);
                    previous = next;
                }
                Ok(Val::Bool(conjoin(&parts)))
            }
            ExprKind::Membership {
                value,
                container,
                negated,
                ..
            } => {
                let item = self.spec(value, env, wd, depth)?;
                let collection = self.spec(container, env, wd, depth)?;
                let held = self.membership(&item, &collection)?;
                Ok(Val::Bool(if *negated { not(&held) } else { held }))
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                let Val::Bool(condition) = self.spec(condition, env, wd, depth)? else {
                    return Err("a condition is not a modeled `bool`".into());
                };
                let mut then_wd = Vec::new();
                let then_value = self.spec(then_expr, env, &mut then_wd, depth)?;
                let mut else_wd = Vec::new();
                let else_value = self.spec(else_expr, env, &mut else_wd, depth)?;
                wd.push(format!("(=> {condition} {})", conjoin(&then_wd)));
                wd.push(format!("(=> {} {})", not(&condition), conjoin(&else_wd)));
                match (then_value, else_value) {
                    (Val::Int(a, ty), Val::Int(b, _)) => {
                        Ok(Val::Int(format!("(ite {condition} {a} {b})"), ty))
                    }
                    (Val::Bool(a), Val::Bool(b)) => {
                        Ok(Val::Bool(format!("(ite {condition} {a} {b})")))
                    }
                    (Val::Str(a), Val::Str(b)) => {
                        Ok(Val::Str(format!("(ite {condition} {a} {b})")))
                    }
                    (Val::Float(a, aty), Val::Float(b, bty)) => {
                        let float = wider_of(aty, bty);
                        Ok(Val::Float(
                            format!(
                                "(ite {condition} {} {})",
                                float.convert(&a, aty),
                                float.convert(&b, bty)
                            ),
                            float,
                        ))
                    }
                    _ => Err("a conditional of this type is not modeled yet".into()),
                }
            }
            ExprKind::Index { object, index } => {
                let object = self.spec(object, env, wd, depth)?;
                match object {
                    Val::List { ref len, .. } => {
                        let Val::Int(index, _) = self.spec(index, env, wd, depth)? else {
                            return Err("a list index is not a modeled integer".into());
                        };
                        wd.push(format!("(and (<= (- {len}) {index}) (< {index} {len}))"));
                        let len = len.clone();
                        // Under a quantifier the index names a bound
                        // variable, which no declared constant may use.
                        if index.contains("!q") {
                            let Val::List {
                                data: Some(data),
                                elem,
                                ..
                            } = &object
                            else {
                                return Err("elements of this type are not modeled yet".into());
                            };
                            let index = format!("(ite (< {index} 0) (+ {index} {len}) {index})");
                            return Ok(self.store_select(data, elem, &index));
                        }
                        let index = self.normalize_index(&len, &index);
                        match self.element(&object, &index, "element") {
                            Val::Opaque => Err("elements of this type are not modeled yet".into()),
                            value => Ok(value),
                        }
                    }
                    Val::Map {
                        keys,
                        values,
                        value,
                        ..
                    } => {
                        let key = match self.spec(index, env, wd, depth)? {
                            Val::Int(term, _) | Val::Str(term) => term,
                            _ => return Err("a dict key is not a modeled value".into()),
                        };
                        wd.push(format!("(select {keys} {key})"));
                        let Some(values) = values else {
                            return Err("values of this type are not modeled yet".into());
                        };
                        let read = scalar_val(format!("(select {values} {key})"), &value);
                        // Under a quantifier the key names a bound
                        // variable, which no declared constant may use.
                        Ok(if key.contains("!q") {
                            read
                        } else {
                            self.name_scalars(read, "value")
                        })
                    }
                    Val::Tuple(items) => match &index.kind {
                        ExprKind::Int(position) => items
                            .get(*position as usize)
                            .cloned()
                            .ok_or("tuple index out of range".to_string()),
                        _ => Err("a tuple index must be a literal".into()),
                    },
                    _ => Err("indexing this type is not modeled in contracts yet".into()),
                }
            }
            ExprKind::Member { object, field } => match self.spec(object, env, wd, depth)? {
                Val::Record(_, fields) => fields
                    .iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, value)| value.clone())
                    .ok_or(format!("field `{field}` is not modeled")),
                _ => Err(format!("`.{field}` on this value is not modeled yet")),
            },
            ExprKind::Call { callee, args } => {
                if let ExprKind::Member { object, field } = &callee.kind {
                    // A `spec def` method of the receiver's class.
                    let receiver = self.spec(object, env, wd, depth)?;
                    if let (Val::List { .. }, "contains", [arg]) =
                        (&receiver, field.as_str(), args.as_slice())
                    {
                        let item = self.spec(&arg.value, env, wd, depth)?;
                        return self.membership(&item, &receiver).map(Val::Bool);
                    }
                    if let Val::Str(text) = &receiver {
                        let [arg] = args.as_slice() else {
                            return Err(format!("`str.{field}` is not modeled in contracts yet"));
                        };
                        let Val::Str(other) = self.spec(&arg.value, env, wd, depth)? else {
                            return Err(format!("`str.{field}` is not modeled in contracts yet"));
                        };
                        return match field.as_str() {
                            "starts_with" => {
                                Ok(Val::Bool(format!("(str.prefixof {other} {text})")))
                            }
                            "ends_with" => Ok(Val::Bool(format!("(str.suffixof {other} {text})"))),
                            "contains" => Ok(Val::Bool(format!("(str.contains {text} {other})"))),
                            _ => Err(format!("`str.{field}` is not modeled in contracts yet")),
                        };
                    }
                    let Val::Record(class, _) = &receiver else {
                        return Err(format!(
                            "`.{field}(...)` on this value is not modeled in contracts yet"
                        ));
                    };
                    let Some(spec) = self.cx.spec_def(&format!("{class}.{field}")) else {
                        return Err(format!(
                            "`{class}.{field}` is not a `spec def`; it is not modeled in contracts"
                        ));
                    };
                    if args.iter().any(|arg| arg.name.is_some()) {
                        return Err("keyword arguments in contracts are not modeled yet".into());
                    }
                    let mut values = vec![receiver];
                    for arg in args {
                        values.push(self.spec(&arg.value, env, wd, depth)?);
                    }
                    return self.spec_call(spec, values, wd, depth);
                }
                let ExprKind::Name(name) = &callee.kind else {
                    return Err("only `len`, `abs`, `min`, `max`, and `spec def` calls are modeled in contracts yet".into());
                };
                if args.iter().any(|arg| arg.name.is_some()) {
                    return Err("keyword arguments in contracts are not modeled yet".into());
                }
                if let ("all" | "any", [arg]) = (name.as_str(), args.as_slice()) {
                    return self.quantifier(name == "all", &arg.value, env, wd, depth);
                }
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.spec(&arg.value, env, wd, depth)?);
                }
                match (name.as_str(), values.as_slice()) {
                    (
                        "len",
                        [Val::List { len, .. } | Val::Map { len, .. } | Val::Set { len, .. }],
                    ) => Ok(Val::Int(len.clone(), IntTy::INT64)),
                    ("len", [Val::Str(text)]) => {
                        Ok(Val::Int(format!("(str.len {text})"), IntTy::INT64))
                    }
                    ("abs", [Val::Float(term, float)]) => {
                        Ok(Val::Float(format!("(fp.abs {term})"), *float))
                    }
                    ("abs", [Val::Int(term, ty)]) => {
                        let result = format!("(ite (< {term} 0) (- {term}) {term})");
                        wd.push(ty.contains(&result));
                        Ok(Val::Int(result, *ty))
                    }
                    ("min" | "max", [Val::Int(a, ty), Val::Int(b, _)]) => {
                        let op = if name == "min" { "<=" } else { ">=" };
                        Ok(Val::Int(format!("(ite ({op} {a} {b}) {a} {b})"), *ty))
                    }
                    _ => {
                        let Some(spec) = self.cx.spec_def(name) else {
                            return Err(format!("`{name}` is not modeled in contracts yet"));
                        };
                        self.spec_call(spec, values, wd, depth)
                    }
                }
            }
            _ => Err("this expression is not modeled in contracts yet".into()),
        }
    }

    /// Whether `item` is in `collection`: a key of a `dict`, an element of
    /// a `set` or a list, or a part of a `str`.
    fn membership(&mut self, item: &Val, collection: &Val) -> Result<String, String> {
        match (item, collection) {
            (Val::Int(key, _) | Val::Str(key), Val::Map { keys, key: ty, .. })
                if key_sort(ty).is_some() =>
            {
                Ok(format!("(select {keys} {key})"))
            }
            (Val::Int(key, _) | Val::Str(key), Val::Set { members, .. }) => {
                Ok(format!("(select {members} {key})"))
            }
            (Val::Str(part), Val::Str(text)) => Ok(format!("(str.contains {text} {part})")),
            (
                _,
                Val::List {
                    len,
                    data: Some(data),
                    elem,
                },
            ) => {
                let k = self.bound_name();
                let element = self.store_select(data, elem, &k);
                let equal = structural_eq(item, &element)
                    .ok_or("membership of this value is not modeled yet".to_string())?;
                Ok(format!(
                    "(exists (({k} Int)) (and (<= 0 {k}) (< {k} {len}) {equal}))"
                ))
            }
            _ => Err("membership in this collection is not modeled in contracts yet".into()),
        }
    }

    pub(super) fn spec_call(
        &mut self,
        spec: &FunctionDecl,
        values: Vec<Val>,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<Val, String> {
        if depth >= MAX_SPEC_DEPTH {
            return Err(format!(
                "`spec def {}` unfolds more than {MAX_SPEC_DEPTH} levels",
                spec.name
            ));
        }
        let names: Vec<String> = spec
            .receiver
            .map(|_| "self".to_string())
            .into_iter()
            .chain(spec.params.iter().map(|param| param.name.clone()))
            .collect();
        if values.len() != names.len() {
            return Err(format!("`spec def {}` takes default arguments", spec.name));
        }
        let env: Env = names.into_iter().zip(values).collect();
        // Each `requires` must hold where the `spec def` is used.
        for clause in &spec.contracts {
            match self.spec(&clause.condition, &env, wd, depth + 1)? {
                Val::Bool(term) => wd.push(term),
                _ => return Err("a `spec def` `requires` is not a modeled `bool`".into()),
            }
        }
        let body = spec
            .body
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::Return(ret) => ret.value.as_ref(),
                _ => None,
            })
            .expect("a checked `spec def` body is one `return`");
        self.spec(body, &env, wd, depth + 1)
    }

    fn spec_binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        env: &Env,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<Val, String> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            let Val::Bool(a) = self.spec(left, env, wd, depth)? else {
                return Err("a logical operand is not a modeled `bool`".into());
            };
            let mut right_wd = Vec::new();
            let Val::Bool(b) = self.spec(right, env, &mut right_wd, depth)? else {
                return Err("a logical operand is not a modeled `bool`".into());
            };
            let guard = if op == BinaryOp::And {
                a.clone()
            } else {
                not(&a)
            };
            if !right_wd.is_empty() {
                wd.push(format!("(=> {guard} {})", conjoin(&right_wd)));
            }
            let symbol = if op == BinaryOp::And { "and" } else { "or" };
            return Ok(Val::Bool(format!("({symbol} {a} {b})")));
        }
        let lhs = self.spec(left, env, wd, depth)?;
        let rhs = self.spec(right, env, wd, depth)?;
        match op {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Less
            | BinaryOp::LessEq
            | BinaryOp::Greater
            | BinaryOp::GreaterEq => compare(op, &lhs, &rhs).map(Val::Bool),
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
                if matches!((&lhs, &rhs), (Val::Float(..), Val::Float(..))) =>
            {
                let (Val::Float(a, aty), Val::Float(b, bty)) = (&lhs, &rhs) else {
                    unreachable!("matched above")
                };
                let float = wider_of(*aty, *bty);
                let (a, b) = (float.convert(a, *aty), float.convert(b, *bty));
                let symbol = match op {
                    BinaryOp::Add => "fp.add",
                    BinaryOp::Sub => "fp.sub",
                    BinaryOp::Mul => "fp.mul",
                    _ => {
                        wd.push(format!("(not (fp.isZero {b}))"));
                        "fp.div"
                    }
                };
                Ok(Val::Float(format!("({symbol} RNE {a} {b})"), float))
            }
            BinaryOp::Add if matches!((&lhs, &rhs), (Val::Str(_), Val::Str(_))) => {
                let (Val::Str(a), Val::Str(b)) = (&lhs, &rhs) else {
                    unreachable!("matched above")
                };
                Ok(Val::Str(format!("(str.++ {a} {b})")))
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::FloorDiv | BinaryOp::Mod => {
                let (Val::Int(a, aty), Val::Int(b, bty)) = (&lhs, &rhs) else {
                    return Err("arithmetic on these operands is not modeled yet".into());
                };
                let ty = if matches!(left.kind, ExprKind::Int(_)) {
                    *bty
                } else {
                    *aty
                };
                let term = match op {
                    BinaryOp::Add => format!("(+ {a} {b})"),
                    BinaryOp::Sub => format!("(- {a} {b})"),
                    BinaryOp::Mul => format!("(* {a} {b})"),
                    _ => {
                        wd.push(format!("(not (= {b} 0))"));
                        division_term(op, a, b)
                    }
                };
                wd.push(ty.contains(&term));
                Ok(Val::Int(term, ty))
            }
            _ => Err(format!("`{op:?}` is not modeled in contracts yet")),
        }
    }
}

impl Exec<'_> {
    /// `all(...)` or `any(...)` of a list comprehension with one `for`
    /// clause over a list of scalars or a `range`, as a quantifier over the
    /// positions: `forall(x in xs, c)` and `exists(x in xs, c)` are written
    /// this way.
    fn quantifier(
        &mut self,
        universal: bool,
        arg: &Expr,
        env: &Env,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<Val, String> {
        let ExprKind::Comprehension {
            output: ComprehensionOutput::List(body),
            clauses,
        } = &arg.kind
        else {
            return Err("`all` and `any` are modeled over a list comprehension".into());
        };
        // Each `for` clause binds one more variable, over positions that may
        // depend on the variables before it.
        let mut ks: Vec<String> = Vec::new();
        let mut hypotheses: Vec<String> = Vec::new();
        let mut inner = env.clone();
        for clause in clauses {
            let k = self.bound_name();
            let mut iterable_wd = Vec::new();
            let (range, value, sort) =
                self.quantified_values(&clause.iterable, &k, &inner, &mut iterable_wd, depth)?;
            push_quantified_wd(wd, &ks, &hypotheses, &iterable_wd);
            ks.push(format!("({k} {sort})"));
            // An element of a list of integers lies in its type's range.
            hypotheses.push(range);
            int_ranges(&value, &mut hypotheses);
            bind_target(&clause.target, value, &mut inner)?;
            // Each filter, then the condition, is evaluated only where the
            // filters before it hold, so each is well-defined there.
            for filter in &clause.filters {
                let mut filter_wd = Vec::new();
                let Val::Bool(term) = self.spec(filter, &inner, &mut filter_wd, depth)? else {
                    return Err("a comprehension filter is not a modeled `bool`".into());
                };
                push_quantified_wd(wd, &ks, &hypotheses, &filter_wd);
                hypotheses.push(term);
            }
        }
        let mut body_wd = Vec::new();
        let Val::Bool(condition) = self.spec(body, &inner, &mut body_wd, depth)? else {
            return Err("a quantified condition is not a modeled `bool`".into());
        };
        push_quantified_wd(wd, &ks, &hypotheses, &body_wd);
        let binders = binders(&ks);
        let hypotheses = conjoin(&hypotheses);
        Ok(Val::Bool(if universal {
            format!("(forall ({binders}) (=> {hypotheses} {condition}))")
        } else {
            format!("(exists ({binders}) (and {hypotheses} {condition}))")
        }))
    }

    /// The positions a quantifier's `for` clause ranges over, as a condition
    /// on the bound variable `k`, and the value its target takes at `k`:
    /// `range(...)`, a modeled list, or `enumerate` of one.
    fn quantified_values(
        &mut self,
        iterable: &Expr,
        k: &str,
        env: &Env,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<(String, Val, &'static str), String> {
        if let ExprKind::Call { callee, args } = &iterable.kind {
            if let ExprKind::Member { object, field } = &callee.kind {
                // `for k in d.keys()` ranges over the keys, as `for k in d`.
                if field == "keys" && args.is_empty() {
                    return self.quantified_list(object, k, env, wd, depth);
                }
            }
            let ExprKind::Name(callee) = &callee.kind else {
                return self.quantified_list(iterable, k, env, wd, depth);
            };
            if callee == "range"
                && (1..=2).contains(&args.len())
                && args.iter().all(|arg| arg.name.is_none())
            {
                let mut bounds = Vec::new();
                for arg in args {
                    let Val::Int(term, _) = self.spec(&arg.value, env, wd, depth)? else {
                        return Err("a `range` bound is not a modeled integer".into());
                    };
                    bounds.push(term);
                }
                let (low, high) = match bounds.as_slice() {
                    [high] => ("0".to_string(), high.clone()),
                    [low, high] => (low.clone(), high.clone()),
                    _ => unreachable!("one or two bounds"),
                };
                return Ok((
                    format!("(and (<= {low} {k}) (< {k} {high}))"),
                    Val::Int(k.to_string(), IntTy::INT64),
                    "Int",
                ));
            }
            if callee == "zip" && args.len() >= 2 && args.iter().all(|arg| arg.name.is_none()) {
                let mut ranges = Vec::new();
                let mut values = Vec::new();
                for arg in args {
                    let (range, value, sort) =
                        self.quantified_list(&arg.value, k, env, wd, depth)?;
                    if sort != "Int" {
                        return Err("`zip` over a `dict` or a `set` is not modeled yet".into());
                    }
                    ranges.push(range);
                    values.push(value);
                }
                return Ok((conjoin(&ranges), Val::Tuple(values), "Int"));
            }
            if callee == "enumerate" && (1..=2).contains(&args.len()) && args[0].name.is_none() {
                let start = match args.get(1) {
                    Some(arg) if matches!(arg.name.as_deref(), None | Some("start")) => {
                        let Val::Int(term, _) = self.spec(&arg.value, env, wd, depth)? else {
                            return Err("an `enumerate` start is not a modeled integer".into());
                        };
                        term
                    }
                    Some(_) => return Err("this `enumerate` call is not modeled yet".into()),
                    None => "0".to_string(),
                };
                let (range, value, sort) =
                    self.quantified_list(&args[0].value, k, env, wd, depth)?;
                if sort != "Int" {
                    return Err("`enumerate` over a `dict` or a `set` is not modeled yet".into());
                }
                let position = if start == "0" {
                    k.to_string()
                } else {
                    format!("(+ {start} {k})")
                };
                return Ok((
                    range,
                    Val::Tuple(vec![Val::Int(position, IntTy::INT64), value]),
                    "Int",
                ));
            }
        }
        self.quantified_list(iterable, k, env, wd, depth)
    }

    fn quantified_list(
        &mut self,
        iterable: &Expr,
        k: &str,
        env: &Env,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<(String, Val, &'static str), String> {
        match self.spec(iterable, env, wd, depth)? {
            Val::List {
                len,
                data: Some(data),
                elem,
            } => {
                let value = self.store_select(&data, &elem, k);
                Ok((format!("(and (<= 0 {k}) (< {k} {len}))"), value, "Int"))
            }
            // A `for` over a `dict` takes its keys, and over a `set` its
            // elements: the bound variable ranges over what it holds.
            Val::Map { keys, key: ty, .. }
            | Val::Set {
                members: keys,
                elem: ty,
                ..
            } => {
                let sort = key_sort(&ty).expect("a modeled collection has a modeled key");
                Ok((
                    format!("(select {keys} {k})"),
                    scalar_val(k.to_string(), &ty),
                    sort,
                ))
            }
            _ => Err("quantifiers over this collection are not modeled yet".into()),
        }
    }
}

/// Binds a quantifier's `for` target to `value`, item by item for a tuple
/// target.
fn bind_target(target: &BindingTarget, value: Val, env: &mut Env) -> Result<(), String> {
    match (target, value) {
        (BindingTarget::Name { name, .. }, value) => {
            if name != "_" {
                env.insert(name.clone(), value);
            }
            Ok(())
        }
        (BindingTarget::Tuple { elements, .. }, Val::Tuple(items))
            if elements.len() == items.len() =>
        {
            for (element, item) in elements.iter().zip(items) {
                bind_target(element, item, env)?;
            }
            Ok(())
        }
        _ => Err("this quantifier target is not modeled yet".into()),
    }
}

/// The SMT binder list of the bound variables `ks`.
fn binders(ks: &[String]) -> String {
    ks.join(" ")
}

/// Records that `terms` hold at every position `ks` where `hypotheses` do.
fn push_quantified_wd(
    wd: &mut Vec<String>,
    ks: &[String],
    hypotheses: &[String],
    terms: &[String],
) {
    if terms.is_empty() {
        return;
    }
    if ks.is_empty() {
        wd.extend(terms.iter().cloned());
        return;
    }
    wd.push(format!(
        "(forall ({}) (=> {} {}))",
        binders(ks),
        conjoin(hypotheses),
        conjoin(terms)
    ));
}

/// The SMT term of an integer `//` or `%` with a non-zero divisor: `//`
/// floors, and `%` takes the divisor's sign. Integers have no `/`.
pub(super) fn division_term(op: BinaryOp, a: &str, b: &str) -> String {
    let floor = format!("(ite (> {b} 0) (div {a} {b}) (div (- {a}) (- {b})))");
    if op == BinaryOp::FloorDiv {
        floor
    } else {
        format!("(- {a} (* {b} {floor}))")
    }
}

/// The SMT term of the comparison `lhs op rhs`.
fn compare(op: BinaryOp, lhs: &Val, rhs: &Val) -> Result<String, String> {
    let symbol = match op {
        BinaryOp::Less => "<",
        BinaryOp::LessEq => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEq => ">=",
        _ => "=",
    };
    let term = match (lhs, rhs) {
        (Val::Int(a, _), Val::Int(b, _)) => format!("({symbol} {a} {b})"),
        (Val::Bool(a), Val::Bool(b)) if symbol == "=" => format!("(= {a} {b})"),
        (Val::Float(a, aty), Val::Float(b, bty)) => {
            let float = wider_of(*aty, *bty);
            let (a, b) = (float.convert(a, *aty), float.convert(b, *bty));
            let symbol = match op {
                BinaryOp::Less => "fp.lt",
                BinaryOp::LessEq => "fp.leq",
                BinaryOp::Greater => "fp.gt",
                BinaryOp::GreaterEq => "fp.geq",
                _ => "fp.eq",
            };
            format!("({symbol} {a} {b})")
        }
        (Val::Str(a), Val::Str(b)) => match op {
            BinaryOp::Less => format!("(str.< {a} {b})"),
            BinaryOp::LessEq => format!("(str.<= {a} {b})"),
            BinaryOp::Greater => format!("(str.< {b} {a})"),
            BinaryOp::GreaterEq => format!("(str.<= {b} {a})"),
            _ => format!("(= {a} {b})"),
        },
        (Val::Map { .. } | Val::Set { .. }, _) if symbol == "=" => structural_eq(lhs, rhs)
            .ok_or("comparing these values is not modeled in contracts yet".to_string())?,
        _ => return Err("comparing these values is not modeled in contracts yet".into()),
    };
    Ok(if op == BinaryOp::NotEq {
        not(&term)
    } else {
        term
    })
}

/// The type two float operands meet at: a `float64` literal beside a
/// `float32` rounds to it.
fn wider_of(a: FloatTy, b: FloatTy) -> FloatTy {
    if a == FloatTy::F32 || b == FloatTy::F32 {
        FloatTy::F32
    } else {
        FloatTy::F64
    }
}

/// A MIR function name without the type arguments of a specialized copy.
pub(super) fn base_name(name: &str) -> &str {
    name.split('[').next().unwrap_or(name)
}

fn public_name(name: &str) -> String {
    crate::mir::public_function_name(name)
}

fn is_invariant_check(callee: &str) -> bool {
    base_name(callee).ends_with(&format!(".{}", crate::ast::CLASS_INVARIANT_METHOD))
}

impl<'a> Exec<'a> {
    /// A `Branch` into the failure block `fail` of an `assert`, `ensures`,
    /// or loop `invariant`, read from the clause as written. The lowered
    /// condition of a quantified clause is a loop the walk only
    /// approximates, so when the contract model reads the clause, the check
    /// is proved from it directly and the walk continues at `pass` with the
    /// clause known. `None` leaves the branch to the lowered code.
    pub(super) fn clause_branch(
        &mut self,
        state: &State,
        fail: usize,
        pass: usize,
    ) -> Option<Vec<(usize, State)>> {
        let crate::mir::Terminator::AssertFail {
            message,
            span,
            kind,
            ..
        } = &self.function.blocks[fail].terminator
        else {
            return None;
        };
        let obligation = match kind {
            crate::mir::AssertKind::Assert => ObligationKind::Assert,
            crate::mir::AssertKind::Ensures => ObligationKind::Ensures,
            crate::mir::AssertKind::Invariant => ObligationKind::LoopInvariant,
            _ => return None,
        };
        let decl = self.cx.decl(&self.function.name)?;
        let condition = find_condition(&decl.contracts, &decl.body, *span)?;
        let mut state = state.clone();
        let env = self.clause_env(&mut state, fail, condition);
        let mut wd = Vec::new();
        let Ok(Val::Bool(term)) = self.spec(condition, &env, &mut wd, 0) else {
            return None;
        };
        if !self.assumed_fails.contains(&fail) {
            let text = super::exec::check_message(*kind, message.as_ref());
            wd.push(term.clone());
            let goal = conjoin(&wd);
            let pc = state.pc.clone();
            self.oblige(obligation, *span, text, &pc, Goal::Holds(goal));
            if self.prove_sites {
                self.copy_as_site((fail, super::exec::CHECK_FAILURE_SITE));
            }
        }
        let pc = state.pc.clone();
        self.assume(&pc, term);
        Some(vec![(pass, state)])
    }

    /// The value of each name `condition` reads, from `state`. The failure
    /// block `fail` names the local behind each renamed source name in
    /// scope; any other name is its own local.
    fn clause_env(&mut self, state: &mut State, fail: usize, condition: &Expr) -> Env {
        let renamed: BTreeMap<&str, &str> = self.function.blocks[fail]
            .instructions
            .iter()
            .filter_map(|instruction| match instruction {
                crate::mir::Instruction::SourceName { local, name } => {
                    Some((name.as_str(), local.as_str()))
                }
                _ => None,
            })
            .collect();
        let mut env = Env::new();
        for name in free_names(condition) {
            let local = renamed
                .get(name.as_str())
                .map_or(name.as_str(), |local| local)
                .to_string();
            if self.place_type(&local).is_none() {
                // A name the clause binds itself, or a function.
                continue;
            }
            let value = self.read_place(state, &local);
            env.insert(name, value);
        }
        env
    }
}

/// The names `expr` reads, as its debug form spells them.
fn free_names(expr: &Expr) -> std::collections::BTreeSet<String> {
    let text = format!("{:?}", expr.kind);
    let mut names = std::collections::BTreeSet::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("Name(\"") {
        rest = &rest[start + 6..];
        if let Some(end) = rest.find('"') {
            names.insert(rest[..end].to_string());
            rest = &rest[end..];
        }
    }
    names
}

/// The condition of the `assert` or clause whose check reports at `span`.
fn find_condition<'a>(
    contracts: &'a [ContractClause],
    body: &'a [Stmt],
    span: Span,
) -> Option<&'a Expr> {
    let clause = |clause: &'a ContractClause| {
        (clause.condition.span == span || clause.span == span).then_some(&clause.condition)
    };
    if let Some(found) = contracts.iter().find_map(clause) {
        return Some(found);
    }
    body.iter().find_map(|stmt| match stmt {
        Stmt::Assert(assert) => match &assert.clause {
            Some(inner) => clause(inner),
            None => {
                (assert.condition.span == span || assert.span == span).then_some(&assert.condition)
            }
        },
        Stmt::If(stmt) => stmt
            .branches
            .iter()
            .find_map(|branch| find_condition(&[], &branch.body, span))
            .or_else(|| {
                stmt.else_body
                    .as_deref()
                    .and_then(|body| find_condition(&[], body, span))
            }),
        Stmt::Match(stmt) => stmt
            .arms
            .iter()
            .find_map(|arm| find_condition(&[], &arm.body, span)),
        Stmt::For(stmt) => find_condition(&stmt.contracts, &stmt.body, span),
        Stmt::While(stmt) => find_condition(&stmt.contracts, &stmt.body, span),
        Stmt::With(stmt) => find_condition(&[], &stmt.body, span),
        _ => None,
    })
}

/// The range facts of each integer a value holds.
fn int_ranges(value: &Val, facts: &mut Vec<String>) {
    match value {
        Val::Int(term, int) => facts.push(int.contains(term)),
        Val::Tuple(items) => items.iter().for_each(|item| int_ranges(item, facts)),
        Val::Record(_, fields) => fields
            .iter()
            .for_each(|(_, value)| int_ranges(value, facts)),
        _ => {}
    }
}
