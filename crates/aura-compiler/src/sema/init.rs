//! Definite initialization of `self` inside a class initializer (ADR-0059).
//!
//! Each field of the object under construction is `Uninit`, `Init`, or
//! `MaybeInit` at every program point. A field with a declaration default is
//! `Init` at entry; every other field starts `Uninit`. The analysis follows
//! the move checker's join and loop rules: a join keeps `Init` only when every
//! incoming path initialized the field, and a loop body is checked against
//! the fixed point of its entry and back edges.
//!
//! The checker rejects, with `AU3012`, a read of a field that is not `Init`,
//! an assignment to a `MaybeInit` field, any use of `self` as a whole before
//! every field is `Init`, and a completion path that leaves a field
//! uninitialized. A `return` with a value is `AU2016`.

use super::{
    AssignTarget, BTreeMap, ClassDecl, Diagnostic, Expr, ExprKind, FunctionDecl, Result, Stmt,
};
use crate::ast::{ComprehensionOutput, FormatPart};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FieldState {
    Uninit,
    Init,
    MaybeInit,
}

impl FieldState {
    fn join(self, other: Self) -> Self {
        if self == other {
            self
        } else {
            Self::MaybeInit
        }
    }
}

/// Field states at one program point, in field declaration order.
type State = Vec<FieldState>;

fn join_states(left: Option<State>, right: Option<State>) -> Option<State> {
    match (left, right) {
        (Some(left), Some(right)) => Some(
            left.iter()
                .zip(right.iter())
                .map(|(left, right)| left.join(*right))
                .collect(),
        ),
        (Some(state), None) | (None, Some(state)) => Some(state),
        (None, None) => None,
    }
}

#[derive(Default)]
struct LoopExits {
    breaks: Option<State>,
    continues: Option<State>,
}

struct InitializerChecker<'a> {
    fields: Vec<&'a str>,
    field_index: BTreeMap<&'a str, usize>,
    loops: Vec<LoopExits>,
}

/// Checks the body of a validated `__init__` against the class's fields.
pub(super) fn check_initializer_definite_initialization(
    class: &ClassDecl,
    initializer: &FunctionDecl,
) -> Result<()> {
    let fields = class
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    let field_index = fields
        .iter()
        .enumerate()
        .map(|(index, name)| (*name, index))
        .collect();
    let entry = class
        .fields
        .iter()
        .map(|field| {
            if field.default.is_some() {
                FieldState::Init
            } else {
                FieldState::Uninit
            }
        })
        .collect::<State>();
    let mut checker = InitializerChecker {
        fields,
        field_index,
        loops: Vec::new(),
    };
    if let Some(exit) = checker.block(&initializer.body, entry)? {
        checker.require_complete(&exit, initializer.span, "falls through")?;
    }
    Ok(())
}

impl<'a> InitializerChecker<'a> {
    fn block(&mut self, statements: &[Stmt], mut state: State) -> Result<Option<State>> {
        for statement in statements {
            match self.statement(statement, state)? {
                Some(next) => state = next,
                None => return Ok(None),
            }
        }
        Ok(Some(state))
    }

    fn statement(&mut self, statement: &Stmt, mut state: State) -> Result<Option<State>> {
        match statement {
            Stmt::Assign(assign) => {
                self.expr(&assign.value, &state)?;
                match &assign.target {
                    AssignTarget::Member { object, field } if is_self(object) => {
                        let Some(index) = self.field_index.get(field.as_str()).copied() else {
                            return Ok(Some(state));
                        };
                        if assign.op.is_some() {
                            self.require_field(&state, index, assign.span)?;
                            return Ok(Some(state));
                        }
                        match state[index] {
                            FieldState::Uninit => state[index] = FieldState::Init,
                            FieldState::Init => {}
                            FieldState::MaybeInit => {
                                return Err(Diagnostic::coded_at(
                                    "AU3012",
                                    assign.span,
                                    format!(
                                        "field `{field}` may already be initialized; assignment needs a known state"
                                    ),
                                )
                                .with_help(format!(
                                    "initialize `self.{field}` exactly once on every path, or assign it on every path before this point"
                                )));
                            }
                        }
                    }
                    AssignTarget::Member { object, .. } => self.place(object, &state)?,
                    AssignTarget::Index { object, index } => {
                        self.place(object, &state)?;
                        self.expr(index, &state)?;
                    }
                    AssignTarget::Name(_) => {}
                }
                Ok(Some(state))
            }
            Stmt::View(view) => {
                self.expr(&view.source, &state)?;
                Ok(Some(state))
            }
            Stmt::Destructure(destructure) => {
                self.expr(&destructure.value, &state)?;
                Ok(Some(state))
            }
            Stmt::Pass(_) => Ok(Some(state)),
            Stmt::Assert(assert) => {
                self.expr(&assert.condition, &state)?;
                if let Some(message) = &assert.message {
                    self.expr(message, &state)?;
                }
                Ok(Some(state))
            }
            Stmt::Return(return_stmt) => {
                if let Some(value) = &return_stmt.value {
                    return Err(Diagnostic::coded_at(
                        "AU2016",
                        value.span,
                        "initializer `__init__` cannot return a value",
                    )
                    .with_help("use a bare `return`; the class call returns the new object"));
                }
                self.require_complete(&state, return_stmt.span, "returns")?;
                Ok(None)
            }
            Stmt::If(if_stmt) => {
                let mut exit = None;
                for branch in &if_stmt.branches {
                    self.expr(&branch.condition, &state)?;
                    exit = join_states(exit, self.block(&branch.body, state.clone())?);
                }
                let otherwise = match &if_stmt.else_body {
                    Some(body) => self.block(body, state)?,
                    None => Some(state),
                };
                Ok(join_states(exit, otherwise))
            }
            Stmt::Match(match_stmt) => {
                self.expr(&match_stmt.scrutinee, &state)?;
                let mut exit = None;
                for arm in &match_stmt.arms {
                    if let Some(guard) = &arm.guard {
                        self.expr(guard, &state)?;
                    }
                    exit = join_states(exit, self.block(&arm.body, state.clone())?);
                }
                Ok(exit)
            }
            Stmt::For(for_stmt) => {
                self.expr(&for_stmt.iterable, &state)?;
                for clause in &for_stmt.contracts {
                    self.expr(&clause.condition, &state)?;
                }
                self.repeat(&for_stmt.body, state, None)
            }
            Stmt::While(while_stmt) => {
                for clause in &while_stmt.contracts {
                    self.expr(&clause.condition, &state)?;
                }
                let infinite = matches!(while_stmt.condition.kind, ExprKind::Bool(true));
                self.repeat(
                    &while_stmt.body,
                    state,
                    Some((&while_stmt.condition, infinite)),
                )
            }
            Stmt::With(with_stmt) => {
                self.expr(&with_stmt.value, &state)?;
                self.block(&with_stmt.body, state)
            }
            Stmt::Break(_) => {
                if let Some(exits) = self.loops.last_mut() {
                    exits.breaks = join_states(exits.breaks.take(), Some(state));
                }
                Ok(None)
            }
            Stmt::Continue(_) => {
                if let Some(exits) = self.loops.last_mut() {
                    exits.continues = join_states(exits.continues.take(), Some(state));
                }
                Ok(None)
            }
            Stmt::Expr(expr_stmt) => {
                self.expr(&expr_stmt.expr, &state)?;
                Ok(Some(state))
            }
        }
    }

    /// Checks a loop body at the fixed point of its entry and back edges. A
    /// `while true` loop leaves only through `break`.
    fn repeat(
        &mut self,
        body: &[Stmt],
        entry: State,
        condition: Option<(&Expr, bool)>,
    ) -> Result<Option<State>> {
        let mut head = entry;
        loop {
            if let Some((condition, _)) = condition {
                self.expr(condition, &head)?;
            }
            self.loops.push(LoopExits::default());
            let end = self.block(body, head.clone())?;
            let exits = self.loops.pop().unwrap_or_default();
            let back_edge = join_states(end, exits.continues);
            let next = join_states(Some(head.clone()), back_edge)
                .expect("a loop head always has its entry state");
            if next == head {
                let infinite = condition.is_some_and(|(_, infinite)| infinite);
                let normal_exit = (!infinite).then_some(head);
                return Ok(join_states(normal_exit, exits.breaks));
            }
            head = next;
        }
    }

    fn require_field(&self, state: &State, index: usize, span: crate::diag::Span) -> Result<()> {
        let field = self.fields[index];
        match state[index] {
            FieldState::Init => Ok(()),
            FieldState::Uninit => Err(Diagnostic::coded_at(
                "AU3012",
                span,
                format!("field `{field}` is used before it is initialized"),
            )
            .with_help(format!("assign `self.{field}` before this use"))),
            FieldState::MaybeInit => Err(Diagnostic::coded_at(
                "AU3012",
                span,
                format!("field `{field}` may be uninitialized here"),
            )
            .with_help(format!(
                "assign `self.{field}` on every path that reaches this use"
            ))),
        }
    }

    fn missing_fields(&self, state: &State) -> Vec<&'a str> {
        self.fields
            .iter()
            .zip(state.iter())
            .filter(|(_, state)| **state != FieldState::Init)
            .map(|(field, _)| *field)
            .collect()
    }

    fn require_whole(&self, state: &State, span: crate::diag::Span) -> Result<()> {
        let missing = self.missing_fields(state);
        if missing.is_empty() {
            return Ok(());
        }
        Err(
            Diagnostic::coded_at("AU3012", span, "partially initialized `self` cannot escape")
                .with_help(format!(
                    "initialize {} before using `self` as a whole",
                    describe_fields(&missing)
                )),
        )
    }

    fn require_complete(&self, state: &State, span: crate::diag::Span, exit: &str) -> Result<()> {
        let missing = self.missing_fields(state);
        if missing.is_empty() {
            return Ok(());
        }
        let message = if missing.len() == 1 {
            format!("initializer {exit} without initializing `{}`", missing[0])
        } else {
            format!(
                "initializer {exit} without initializing {}",
                describe_fields(&missing)
            )
        };
        Err(Diagnostic::coded_at("AU3012", span, message).with_help(
            "assign every field without a declaration default on every path through `__init__`",
        ))
    }

    /// A place written through (`self.a.b = ...`, `self.items[i] = ...`):
    /// its root field must already be initialized.
    fn place(&self, object: &Expr, state: &State) -> Result<()> {
        self.expr(object, state)
    }

    fn expr(&self, expr: &Expr, state: &State) -> Result<()> {
        match &expr.kind {
            ExprKind::Name(name) if name == "self" => self.require_whole(state, expr.span),
            ExprKind::Member { object, field } if is_self(object) => {
                match self.field_index.get(field.as_str()) {
                    Some(index) => self.require_field(state, *index, expr.span),
                    // A method, bound method, or any other member reaches the
                    // whole object.
                    None => self.require_whole(state, expr.span),
                }
            }
            ExprKind::Name(_)
            | ExprKind::Int(_)
            | ExprKind::DurationNanos(_)
            | ExprKind::BuiltinOmitted
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_) => Ok(()),
            ExprKind::FString(parts) => {
                for part in parts {
                    if let FormatPart::Expr(expr) | FormatPart::Formatted { expr, .. } = part {
                        self.expr(expr, state)?;
                    }
                }
                Ok(())
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                elements
                    .iter()
                    .try_for_each(|element| self.expr(element, state))
            }
            ExprKind::Map(entries) => entries.iter().try_for_each(|entry| {
                self.expr(&entry.key, state)?;
                self.expr(&entry.value, state)
            }),
            ExprKind::Comprehension { output, clauses } => {
                for clause in clauses {
                    self.expr(&clause.iterable, state)?;
                    for filter in &clause.filters {
                        self.expr(filter, state)?;
                    }
                }
                match output {
                    ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                        self.expr(value, state)
                    }
                    ComprehensionOutput::Map { key, value } => {
                        self.expr(key, state)?;
                        self.expr(value, state)
                    }
                }
            }
            ExprKind::Unary { expr, .. }
            | ExprKind::Cast { expr, .. }
            | ExprKind::Specialize { expr, .. }
            | ExprKind::Try(expr)
            | ExprKind::Group(expr) => self.expr(expr, state),
            ExprKind::Binary { left, right, .. } => {
                self.expr(left, state)?;
                self.expr(right, state)
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.expr(condition, state)?;
                self.expr(then_expr, state)?;
                self.expr(else_expr, state)
            }
            ExprKind::Lambda { body, captures, .. } => {
                // A closure that names `self` captures the whole object.
                if captures
                    .iter()
                    .flatten()
                    .any(|capture| capture.name == "self")
                    || expr_mentions_self(body)
                {
                    return self.require_whole(state, expr.span);
                }
                Ok(())
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee, state)?;
                args.iter()
                    .try_for_each(|argument| self.expr(&argument.value, state))
            }
            ExprKind::Member { object, .. } => self.expr(object, state),
            ExprKind::Index { object, index } => {
                self.expr(object, state)?;
                self.expr(index, state)
            }
            ExprKind::Slice {
                object, start, end, ..
            } => {
                self.expr(object, state)?;
                if let Some(start) = start {
                    self.expr(start, state)?;
                }
                if let Some(end) = end {
                    self.expr(end, state)?;
                }
                Ok(())
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, state)?;
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.expr(guard, state)?;
                    }
                    self.expr(&arm.value, state)?;
                }
                Ok(())
            }
            ExprKind::Membership {
                value, container, ..
            } => {
                self.expr(value, state)?;
                self.expr(container, state)
            }
            ExprKind::CompareChain { first, links } => {
                self.expr(first, state)?;
                links
                    .iter()
                    .try_for_each(|link| self.expr(&link.operand, state))
            }
            ExprKind::IsNone { value, .. } => self.expr(value, state),
        }
    }
}

fn is_self(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Name(name) => name == "self",
        ExprKind::Group(inner) => is_self(inner),
        _ => false,
    }
}

fn describe_fields(fields: &[&str]) -> String {
    let quoted = fields
        .iter()
        .map(|field| format!("`{field}`"))
        .collect::<Vec<_>>();
    match quoted.as_slice() {
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
        [] => String::new(),
    }
}

/// Whether a closure body names `self` anywhere. A lambda parameter cannot be
/// named `self`, so any mention reaches the enclosing initializer's object.
fn expr_mentions_self(expr: &Expr) -> bool {
    let mut found = false;
    visit_expr(expr, &mut |expr| {
        if matches!(&expr.kind, ExprKind::Name(name) if name == "self") {
            found = true;
        }
    });
    found
}

fn visit_expr(expr: &Expr, visit: &mut dyn FnMut(&Expr)) {
    visit(expr);
    match &expr.kind {
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_) => {}
        ExprKind::FString(parts) => {
            for part in parts {
                if let FormatPart::Expr(expr) | FormatPart::Formatted { expr, .. } = part {
                    visit_expr(expr, visit);
                }
            }
        }
        ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
            for element in elements {
                visit_expr(element, visit);
            }
        }
        ExprKind::Map(entries) => {
            for entry in entries {
                visit_expr(&entry.key, visit);
                visit_expr(&entry.value, visit);
            }
        }
        ExprKind::Comprehension { output, clauses } => {
            for clause in clauses {
                visit_expr(&clause.iterable, visit);
                for filter in &clause.filters {
                    visit_expr(filter, visit);
                }
            }
            match output {
                ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                    visit_expr(value, visit)
                }
                ComprehensionOutput::Map { key, value } => {
                    visit_expr(key, visit);
                    visit_expr(value, visit);
                }
            }
        }
        ExprKind::Unary { expr, .. }
        | ExprKind::Cast { expr, .. }
        | ExprKind::Specialize { expr, .. }
        | ExprKind::Try(expr)
        | ExprKind::Group(expr) => visit_expr(expr, visit),
        ExprKind::Binary { left, right, .. } => {
            visit_expr(left, visit);
            visit_expr(right, visit);
        }
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => {
            visit_expr(condition, visit);
            visit_expr(then_expr, visit);
            visit_expr(else_expr, visit);
        }
        ExprKind::Lambda { body, .. } => visit_expr(body, visit),
        ExprKind::Call { callee, args } => {
            visit_expr(callee, visit);
            for argument in args {
                visit_expr(&argument.value, visit);
            }
        }
        ExprKind::Member { object, .. } => visit_expr(object, visit),
        ExprKind::Index { object, index } => {
            visit_expr(object, visit);
            visit_expr(index, visit);
        }
        ExprKind::Slice {
            object, start, end, ..
        } => {
            visit_expr(object, visit);
            if let Some(start) = start {
                visit_expr(start, visit);
            }
            if let Some(end) = end {
                visit_expr(end, visit);
            }
        }
        ExprKind::Match {
            scrutinee, arms, ..
        } => {
            visit_expr(scrutinee, visit);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    visit_expr(guard, visit);
                }
                visit_expr(&arm.value, visit);
            }
        }
        ExprKind::Membership {
            value, container, ..
        } => {
            visit_expr(value, visit);
            visit_expr(container, visit);
        }
        ExprKind::CompareChain { first, links } => {
            visit_expr(first, visit);
            for link in links {
                visit_expr(&link.operand, visit);
            }
        }
        ExprKind::IsNone { value, .. } => visit_expr(value, visit),
    }
}
