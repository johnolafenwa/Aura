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

use super::exec::{conjoin, not, Exec, Goal, IntTy, State, Val};
use super::ObligationKind;
use crate::ast::{
    BinaryOp, CompareOp, ContractClause, ContractKind, Expr, ExprKind, FunctionDecl, Stmt, UnaryOp,
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
        let Some(decl) = self.cx.decl(callee) else {
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
                for clause in clauses {
                    let message = format!(
                        "class invariant holds before `{display}`: `{}`",
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
            }
        }
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
        let Some(decl) = self.cx.decl(callee) else {
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

    /// On entry to a public `mut self` method of a class with invariants,
    /// the invariants hold: every caller proves them.
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

    /// The invariant clauses a call of `callee` must keep, when it is a
    /// public method that takes `mut self` of a class with invariants.
    fn guarded_invariant(&self, callee: &str) -> Option<Vec<ContractClause>> {
        let decl = self.cx.decl(callee)?;
        if !decl.public || decl.receiver != Some(crate::ast::ReceiverKind::BorrowMut) {
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
            ExprKind::Name(name) => env
                .get(name)
                .cloned()
                .ok_or(format!("`{name}` is not modeled in contracts yet")),
            ExprKind::Group(inner) => self.spec(inner, env, wd, depth),
            ExprKind::Unary { op, expr: inner } => {
                let value = self.spec(inner, env, wd, depth)?;
                match (op, value) {
                    (UnaryOp::Not, Val::Bool(term)) => Ok(Val::Bool(not(&term))),
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
                            return Err("membership is not modeled in contracts yet".into())
                        }
                    };
                    parts.push(compare(op, &previous, &next)?);
                    previous = next;
                }
                Ok(Val::Bool(conjoin(&parts)))
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
                        let index = self.normalize_index(&len, &index);
                        match self.element(&object, &index, "element") {
                            Val::Opaque => Err("elements of this type are not modeled yet".into()),
                            value => Ok(value),
                        }
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
                let ExprKind::Name(name) = &callee.kind else {
                    return Err("only `len`, `abs`, `min`, `max`, and `spec def` calls are modeled in contracts yet".into());
                };
                if args.iter().any(|arg| arg.name.is_some()) {
                    return Err("keyword arguments in contracts are not modeled yet".into());
                }
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.spec(&arg.value, env, wd, depth)?);
                }
                match (name.as_str(), values.as_slice()) {
                    ("len", [Val::List { len, .. }]) => Ok(Val::Int(len.clone(), IntTy::INT64)),
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
        if values.len() != spec.params.len() {
            return Err(format!("`spec def {}` takes default arguments", spec.name));
        }
        let body = spec
            .body
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::Return(ret) => ret.value.as_ref(),
                _ => None,
            })
            .expect("a checked `spec def` body is one `return`");
        let env: Env = spec
            .params
            .iter()
            .map(|param| param.name.clone())
            .zip(values)
            .collect();
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
        _ => return Err("comparing these values is not modeled in contracts yet".into()),
    };
    Ok(if op == BinaryOp::NotEq {
        not(&term)
    } else {
        term
    })
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
