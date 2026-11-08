//! Generators passed straight to a function that loops over them.
//!
//! A call such as `total(upto(n))`, where `total` takes an `own
//! Generator[T]` parameter and only loops over it once, hands the function a
//! generator value, so the loop resumes the generator's own stack for every
//! item. This pass gives the call a copy of the function that takes the
//! generator's arguments instead and calls the generator in its loop:
//!
//! ```text
//! def total(source: own Generator[int64]) -> int64:   # unchanged
//!     ...
//!     for x in source:
//!         ...
//!
//! def __aura_with_total__0_upto(__aura_source_0: int64) -> int64:
//!     ...
//!     for x in upto(__aura_source_0):
//!         ...
//!
//! __aura_with_total__0_upto(n)                         # was total(upto(n))
//! ```
//!
//! The loop in the copy is a loop over a direct generator call, which the
//! frame and fusion passes in `iterator_fast.rs` already run without a
//! separate stack. The generator's arguments are evaluated where the call
//! evaluated them, and its body still runs only when the loop starts, so
//! the copy means the same as the call. A generator function that is itself
//! passed a generator gets a copy the same way, so a pipeline such as
//! `total(doubled(upto(n)))` becomes loops over direct calls all the way
//! down.

use super::iterator_fast::{generated_span, renumber};
use super::{BTreeMap, BTreeSet, Item, Module};
use crate::ast::{
    Argument, AssignTarget, BindingTarget, Expr, ExprKind, FunctionDecl, Param, ParamMode, Stmt,
    TypeRef, TypeRefKind,
};

/// Prefix of a function copy that takes a generator's arguments; trap call
/// chains show the copy under the original's name.
pub(crate) use crate::diag::GENERATOR_COPY_PREFIX as WITH_PREFIX;

/// A module-level generator function a call can pass along.
struct Generator {
    item: TypeRef,
    params: Vec<Param>,
}

/// A parameter of a module function that a copy can replace with a
/// generator's arguments: `own Generator[T]`, used once, as the source of a
/// `for` loop that is not inside another loop.
struct Slot {
    item: TypeRef,
}

/// Gives each call that passes a direct generator call to a function that
/// only loops over it a copy of that function. Returns how many calls
/// changed.
pub(super) fn specialize(module: &mut Module, columns: &mut usize) -> usize {
    let mut generators = BTreeMap::new();
    let mut functions = BTreeMap::new();
    for item in &module.items {
        let Item::Function(function) = item else {
            continue;
        };
        if let Some(generator) = generator_of(function) {
            generators.insert(function.name.clone(), generator);
        }
        if function.name.starts_with(WITH_PREFIX) || !plain(function) {
            continue;
        }
        let slots = slots_of(function);
        if !slots.is_empty() {
            functions.insert(function.name.clone(), (function.clone(), slots));
        }
    }
    if functions.is_empty() || generators.is_empty() {
        return 0;
    }
    let mut pass = Specializer {
        generators,
        functions,
        copies: BTreeMap::new(),
        pending: BTreeSet::new(),
        columns,
        count: 0,
    };
    for item in &mut module.items {
        let bodies: Vec<&mut FunctionDecl> = match item {
            Item::Function(function) => vec![function],
            Item::Class(class) => class.methods.iter_mut().collect(),
            Item::Impl(impl_decl) => impl_decl.methods.iter_mut().collect(),
            _ => Vec::new(),
        };
        for function in bodies {
            let bound = bound_names(&function.params, &function.body);
            pass.pass_held(&mut function.body, &bound);
            pass.body(&mut function.body, &bound);
        }
    }
    let bound = bound_names(&[], &module.top_level_stmts);
    pass.pass_held(&mut module.top_level_stmts, &bound);
    pass.body(&mut module.top_level_stmts, &bound);
    let count = pass.count;
    module
        .items
        .extend(pass.copies.into_values().map(Item::Function));
    count
}

/// Whether a copy of `function` can stand in for it: a plain module
/// function with no type parameters, decorators, or contracts.
fn plain(function: &FunctionDecl) -> bool {
    function.type_params.is_empty()
        && function.receiver.is_none()
        && function.decorators.is_empty()
        && function.contracts.is_empty()
        && function.view_return.is_none()
        && function.property.is_none()
        && function
            .params
            .iter()
            .all(|param| param.default.is_none() && !param.keyword_only)
}

fn generator_item(ty: &TypeRef) -> Option<&TypeRef> {
    let TypeRefKind::Named { name, args } = &ty.kind else {
        return None;
    };
    (name.rsplit('.').next() == Some("Generator") && args.len() == 1).then(|| &args[0])
}

fn generator_of(function: &FunctionDecl) -> Option<Generator> {
    let item = generator_item(&function.return_type)?;
    (plain(function) && super::generators::body_yields(&function.body)).then(|| Generator {
        item: item.clone(),
        params: function.params.clone(),
    })
}

/// The parameters of `function` that a copy can replace, by position.
fn slots_of(function: &FunctionDecl) -> BTreeMap<usize, Slot> {
    let mut slots = BTreeMap::new();
    for (index, param) in function.params.iter().enumerate() {
        if param.mode != ParamMode::Own {
            continue;
        }
        let Some(item) = generator_item(&param.ty) else {
            continue;
        };
        let mut uses = 0;
        let mut looped = 0;
        count_uses(&function.body, &param.name, false, &mut uses, &mut looped);
        if uses == 1 && looped == 1 {
            slots.insert(index, Slot { item: item.clone() });
        }
    }
    slots
}

/// Counts every read of `name` in `body` into `uses`, and into `looped`
/// each that is the whole source of a consuming `for` loop outside any
/// other loop.
fn count_uses(body: &[Stmt], name: &str, in_loop: bool, uses: &mut usize, looped: &mut usize) {
    for stmt in body {
        let mut exprs: Vec<&Expr> = Vec::new();
        match stmt {
            Stmt::Assign(assign) => {
                if matches!(&assign.target, AssignTarget::Name(target) if target == name) {
                    *uses += 2;
                }
                match &assign.target {
                    AssignTarget::Name(_) => {}
                    AssignTarget::Member { object, .. } => exprs.push(object),
                    AssignTarget::Index { object, index } => {
                        exprs.push(object);
                        exprs.push(index);
                    }
                }
                exprs.push(&assign.value);
            }
            Stmt::View(view) => {
                if view.name == name {
                    *uses += 2;
                }
                exprs.push(&view.source);
            }
            Stmt::Destructure(destructure) => {
                if binds(&destructure.target, name) {
                    *uses += 2;
                }
                exprs.push(&destructure.value);
            }
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Assert(assert) => {
                exprs.push(&assert.condition);
                exprs.extend(assert.message.as_ref());
            }
            Stmt::Return(ret) => exprs.extend(ret.value.as_ref()),
            Stmt::Expr(expr_stmt) => exprs.push(&expr_stmt.expr),
            Stmt::If(if_stmt) => {
                for branch in &if_stmt.branches {
                    count_expr(&branch.condition, name, uses);
                    count_uses(&branch.body, name, in_loop, uses, looped);
                }
                if let Some(body) = &if_stmt.else_body {
                    count_uses(body, name, in_loop, uses, looped);
                }
            }
            Stmt::Match(match_stmt) => {
                count_expr(&match_stmt.scrutinee, name, uses);
                for arm in &match_stmt.arms {
                    // A pattern binding of the same name would shadow it.
                    if serde_json::to_string(&arm.pattern)
                        .map_or(true, |text| text.contains(&format!("\"{name}\"")))
                    {
                        *uses += 2;
                    }
                    if let Some(guard) = &arm.guard {
                        count_expr(guard, name, uses);
                    }
                    count_uses(&arm.body, name, in_loop, uses, looped);
                }
            }
            Stmt::With(with) => {
                if with.binding == name {
                    *uses += 2;
                }
                count_expr(&with.value, name, uses);
                count_uses(&with.body, name, in_loop, uses, looped);
            }
            Stmt::While(while_stmt) => {
                count_expr(&while_stmt.condition, name, uses);
                if !while_stmt.contracts.is_empty() {
                    *uses += 2;
                }
                count_uses(&while_stmt.body, name, true, uses, looped);
            }
            Stmt::For(for_stmt) => {
                if binds(&for_stmt.target, name) || !for_stmt.contracts.is_empty() {
                    *uses += 2;
                }
                if matches!(&for_stmt.iterable.kind, ExprKind::Name(source) if source == name) {
                    *uses += 1;
                    if !in_loop && for_stmt.borrow_mode.is_none() {
                        *looped += 1;
                    }
                } else {
                    count_expr(&for_stmt.iterable, name, uses);
                }
                count_uses(&for_stmt.body, name, true, uses, looped);
            }
        }
        for expr in exprs {
            count_expr(expr, name, uses);
        }
    }
}

/// Counts reads of `name` in `expr`. A lambda or comprehension that binds
/// the name counts as more than one use, so the parameter is left alone.
fn count_expr(expr: &Expr, name: &str, uses: &mut usize) {
    match &expr.kind {
        ExprKind::Name(found) if found == name => *uses += 1,
        ExprKind::Lambda { .. } | ExprKind::Comprehension { .. } | ExprKind::Match { .. } => {
            if serde_json::to_string(expr)
                .map_or(true, |text| text.contains(&format!("\"{name}\"")))
            {
                *uses += 2;
            }
        }
        _ => {
            let mut probe = expr.clone();
            probe.for_each_child_mut(&mut |child| count_expr(child, name, uses));
        }
    }
}

fn binds(target: &BindingTarget, name: &str) -> bool {
    match target {
        BindingTarget::Name { name: bound, .. } | BindingTarget::Rest { name: bound, .. } => {
            bound == name
        }
        BindingTarget::Tuple { elements, .. } => elements.iter().any(|inner| binds(inner, name)),
    }
}

/// Names a function body may bind: its parameters, and every name its
/// statements and patterns bind, so a call through one of them is not taken
/// for a call of the module function. Each binding form serializes its name
/// as `"name"`, `"binding"`, or an assignment `"target"`, so a scan of the
/// body's text finds them all; it also finds keyword argument names, which
/// only makes the pass more careful.
fn bound_names(params: &[Param], body: &[Stmt]) -> BTreeSet<String> {
    let mut out = params
        .iter()
        .map(|param| param.name.clone())
        .collect::<BTreeSet<_>>();
    let Ok(text) = serde_json::to_string(body) else {
        return out;
    };
    for key in ["\"name\":\"", "\"binding\":\"", "\"target\":{\"Name\":\""] {
        let mut rest = text.as_str();
        while let Some(start) = rest.find(key) {
            let tail = &rest[start + key.len()..];
            let Some(end) = tail.find('"') else {
                break;
            };
            out.insert(tail[..end].to_string());
            rest = &tail[end..];
        }
    }
    out
}

/// Whether two written types are the same, apart from where they are
/// written.
fn same_type(left: &TypeRef, right: &TypeRef) -> bool {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("span");
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let (Ok(mut left), Ok(mut right)) = (serde_json::to_value(left), serde_json::to_value(right))
    else {
        return false;
    };
    strip(&mut left);
    strip(&mut right);
    left == right
}

struct Specializer<'c> {
    generators: BTreeMap<String, Generator>,
    functions: BTreeMap<String, (FunctionDecl, BTreeMap<usize, Slot>)>,
    copies: BTreeMap<String, FunctionDecl>,
    /// Copies being made, so a function that passes itself a generator
    /// does not copy itself forever.
    pending: BTreeSet<String>,
    columns: &'c mut usize,
    count: usize,
}

impl Specializer<'_> {
    /// Moves a held generator call to the one call that passes the local on:
    ///
    /// ```text
    /// g = upto(n)              # becomes   mut <held L:C 0> = n
    /// ...                      #           ...
    /// total(g)                 #           total(upto(<held L:C 0>))
    /// ```
    ///
    /// when the local is read only there, in a later statement of the same
    /// block, and that call can take a copy. The arguments are still
    /// evaluated where they were, and the generator's body runs only when
    /// the copy's loop starts, as it would have.
    fn pass_held(&mut self, body: &mut Vec<Stmt>, bound: &BTreeSet<String>) {
        let Ok(text) = serde_json::to_string(&*body) else {
            return;
        };
        let uses = |name: &str| {
            text.matches(&format!("{{\"Name\":{}}}", serde_json::Value::from(name)))
                .count()
        };
        self.held_block(body, bound, &uses);
    }

    fn held_block(
        &mut self,
        body: &mut Vec<Stmt>,
        bound: &BTreeSet<String>,
        uses: &dyn Fn(&str) -> usize,
    ) {
        for stmt in body.iter_mut() {
            match stmt {
                Stmt::If(if_stmt) => {
                    for branch in &mut if_stmt.branches {
                        self.held_block(&mut branch.body, bound, uses);
                    }
                    if let Some(else_body) = &mut if_stmt.else_body {
                        self.held_block(else_body, bound, uses);
                    }
                }
                Stmt::While(while_stmt) => self.held_block(&mut while_stmt.body, bound, uses),
                Stmt::For(for_stmt) => self.held_block(&mut for_stmt.body, bound, uses),
                Stmt::Match(match_stmt) => {
                    for arm in &mut match_stmt.arms {
                        self.held_block(&mut arm.body, bound, uses);
                    }
                }
                Stmt::With(with) => self.held_block(&mut with.body, bound, uses),
                _ => {}
            }
        }
        let mut index = 0;
        while index < body.len() {
            let Some((held, generator, args, span)) = self.held_call(&body[index], bound) else {
                index += 1;
                continue;
            };
            // The local is assigned once and read once.
            if uses(&held) != 2 {
                index += 1;
                continue;
            }
            let Some(later) =
                (index + 1..body.len()).find(|&later| self.passes_held(&body[later], &held, bound))
            else {
                index += 1;
                continue;
            };
            let mut hidden = Vec::with_capacity(args.len());
            let mut assigns = Vec::with_capacity(args.len());
            for (position, arg) in args.into_iter().enumerate() {
                let name = format!("held {}:{} {position}", span.line, span.column);
                assigns.push(Stmt::Assign(crate::ast::AssignStmt {
                    mutable: true,
                    target: AssignTarget::Name(name.clone()),
                    annotation: None,
                    op: None,
                    value: arg,
                    span,
                }));
                hidden.push(name);
            }
            replace_held(&mut body[later], &held, &generator, &hidden);
            let added = assigns.len();
            body.splice(index..=index, assigns);
            index += added;
        }
    }

    /// `name = generator(args...)` with positional arguments.
    fn held_call(
        &self,
        stmt: &Stmt,
        bound: &BTreeSet<String>,
    ) -> Option<(String, String, Vec<Expr>, crate::diag::Span)> {
        let Stmt::Assign(assign) = stmt else {
            return None;
        };
        let (AssignTarget::Name(held), None, None) =
            (&assign.target, &assign.annotation, &assign.op)
        else {
            return None;
        };
        let ExprKind::Call { callee, args } = &assign.value.kind else {
            return None;
        };
        let ExprKind::Name(generator) = &callee.kind else {
            return None;
        };
        let found = self.generators.get(generator)?;
        (!bound.contains(generator)
            && args.len() == found.params.len()
            && args.iter().all(|arg| arg.name.is_none()))
        .then(|| {
            (
                held.clone(),
                generator.clone(),
                args.iter().map(|arg| arg.value.clone()).collect(),
                assign.span,
            )
        })
    }

    /// Whether `stmt` passes `held` straight to a call that a copy can take,
    /// outside any loop or nested block of the statement.
    fn passes_held(&self, stmt: &Stmt, held: &str, bound: &BTreeSet<String>) -> bool {
        let mut found = false;
        for expr in direct_exprs(stmt) {
            visit_calls(expr, &mut |function, args| {
                if bound.contains(function) {
                    return;
                }
                let Some((decl, slots)) = self.functions.get(function) else {
                    return;
                };
                if args.len() != decl.params.len() || args.iter().any(|arg| arg.name.is_some()) {
                    return;
                }
                found |= slots.keys().any(|&index| {
                    matches!(&args[index].value.kind, ExprKind::Name(name) if name == held)
                });
            });
        }
        found
    }

    fn body(&mut self, body: &mut [Stmt], bound: &BTreeSet<String>) {
        for stmt in body {
            self.stmt(stmt, bound);
        }
    }

    fn stmt(&mut self, stmt: &mut Stmt, bound: &BTreeSet<String>) {
        match stmt {
            Stmt::Assign(assign) => {
                match &mut assign.target {
                    AssignTarget::Name(_) => {}
                    AssignTarget::Member { object, .. } => self.expr(object, bound),
                    AssignTarget::Index { object, index } => {
                        self.expr(object, bound);
                        self.expr(index, bound);
                    }
                }
                self.expr(&mut assign.value, bound);
            }
            Stmt::View(view) => self.expr(&mut view.source, bound),
            Stmt::Destructure(destructure) => self.expr(&mut destructure.value, bound),
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Assert(assert) => {
                self.expr(&mut assert.condition, bound);
                if let Some(message) = &mut assert.message {
                    self.expr(message, bound);
                }
            }
            Stmt::Return(ret) => {
                if let Some(value) = &mut ret.value {
                    self.expr(value, bound);
                }
            }
            Stmt::Expr(expr_stmt) => self.expr(&mut expr_stmt.expr, bound),
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    self.expr(&mut branch.condition, bound);
                    self.body(&mut branch.body, bound);
                }
                if let Some(body) = &mut if_stmt.else_body {
                    self.body(body, bound);
                }
            }
            Stmt::Match(match_stmt) => {
                self.expr(&mut match_stmt.scrutinee, bound);
                for arm in &mut match_stmt.arms {
                    self.body(&mut arm.body, bound);
                }
            }
            Stmt::With(with) => {
                self.expr(&mut with.value, bound);
                self.body(&mut with.body, bound);
            }
            Stmt::While(while_stmt) => {
                self.expr(&mut while_stmt.condition, bound);
                self.body(&mut while_stmt.body, bound);
            }
            Stmt::For(for_stmt) => {
                self.expr(&mut for_stmt.iterable, bound);
                self.body(&mut for_stmt.body, bound);
            }
        }
    }

    fn expr(&mut self, expr: &mut Expr, bound: &BTreeSet<String>) {
        if matches!(
            expr.kind,
            ExprKind::Lambda { .. } | ExprKind::Comprehension { .. }
        ) {
            return;
        }
        expr.for_each_child_mut(&mut |child| self.expr(child, bound));
        let ExprKind::Call { callee, args } = &mut expr.kind else {
            return;
        };
        let ExprKind::Name(function) = &callee.kind else {
            return;
        };
        if bound.contains(function) {
            return;
        }
        let Some(copy) = self.copy_for(function, args, bound) else {
            return;
        };
        let (name, passed) = copy;
        callee.kind = ExprKind::Name(name);
        *args = passed;
        self.count += 1;
    }

    /// The copy of `function` that takes the arguments of the generator
    /// calls among `args`, and the arguments to pass it, when there is one.
    fn copy_for(
        &mut self,
        function: &str,
        args: &[Argument],
        bound: &BTreeSet<String>,
    ) -> Option<(String, Vec<Argument>)> {
        let (decl, slots) = self.functions.get(function)?;
        if args.len() != decl.params.len() || args.iter().any(|arg| arg.name.is_some()) {
            return None;
        }
        // Each replaced parameter, with the generator its argument calls.
        let mut chosen = Vec::new();
        for (index, slot) in slots {
            let ExprKind::Call {
                callee,
                args: inner,
            } = &args[*index].value.kind
            else {
                continue;
            };
            let ExprKind::Name(generator) = &callee.kind else {
                continue;
            };
            if bound.contains(generator) {
                continue;
            }
            let Some(found) = self.generators.get(generator) else {
                continue;
            };
            if same_type(&found.item, &slot.item)
                && inner.len() == found.params.len()
                && inner.iter().all(|arg| arg.name.is_none())
            {
                chosen.push((*index, generator.clone()));
            }
        }
        if chosen.is_empty() {
            return None;
        }
        let mut name = format!("{WITH_PREFIX}{function}");
        for (index, generator) in &chosen {
            name.push_str(&format!("__{index}_{generator}"));
        }
        if !self.copies.contains_key(&name) && self.pending.insert(name.clone()) {
            let decl = decl.clone();
            let copy = self.make_copy(decl, &name, &chosen);
            if let Some(item) = generator_item(&copy.return_type) {
                if super::generators::body_yields(&copy.body) {
                    self.generators.insert(
                        name.clone(),
                        Generator {
                            item: item.clone(),
                            params: copy.params.clone(),
                        },
                    );
                }
            }
            self.copies.insert(name.clone(), copy);
        }
        let mut passed = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            if chosen.iter().any(|(at, _)| *at == index) {
                let ExprKind::Call { args: inner, .. } = &arg.value.kind else {
                    unreachable!("chosen arguments are calls");
                };
                passed.extend(inner.iter().cloned());
            } else {
                passed.push(arg.clone());
            }
        }
        Some((name, passed))
    }

    fn make_copy(
        &mut self,
        mut copy: FunctionDecl,
        name: &str,
        chosen: &[(usize, String)],
    ) -> FunctionDecl {
        copy.name = name.to_string();
        copy.public = false;
        copy.doc = None;
        copy.span = generated_span(copy.span, self.columns);
        let mut params = Vec::new();
        let mut sources = BTreeMap::new();
        for (index, param) in copy.params.iter().enumerate() {
            let Some((_, generator)) = chosen.iter().find(|(at, _)| *at == index) else {
                params.push(param.clone());
                continue;
            };
            let found = &self.generators[generator];
            let mut names = Vec::new();
            for (position, inner) in found.params.iter().enumerate() {
                let mut inner = inner.clone();
                inner.name = format!("__aura_{}_{position}", param.name);
                inner.span = generated_span(param.span, self.columns);
                names.push(inner.name.clone());
                params.push(inner);
            }
            sources.insert(param.name.clone(), (generator.clone(), names));
        }
        copy.params = params;
        point_loops(&mut copy.body, &sources, self.columns);
        renumber(&mut copy.body, self.columns);
        // The copy may itself pass a generator along.
        let bound = bound_names(&copy.params, &copy.body);
        self.body(&mut copy.body, &bound);
        copy
    }
}

/// Points each `for` loop over a replaced parameter at a call of its
/// generator with the copy's new parameters.
fn point_loops(
    body: &mut [Stmt],
    sources: &BTreeMap<String, (String, Vec<String>)>,
    columns: &mut usize,
) {
    for stmt in body {
        match stmt {
            Stmt::For(for_stmt) => {
                if let ExprKind::Name(source) = &for_stmt.iterable.kind {
                    if let Some((generator, names)) = sources.get(source) {
                        let span = generated_span(for_stmt.iterable.span, columns);
                        for_stmt.span = generated_span(for_stmt.span, columns);
                        for_stmt.iterable = Expr {
                            kind: ExprKind::Call {
                                callee: Box::new(Expr {
                                    kind: ExprKind::Name(generator.clone()),
                                    span,
                                }),
                                args: names
                                    .iter()
                                    .map(|name| Argument {
                                        name: None,
                                        value: Expr {
                                            kind: ExprKind::Name(name.clone()),
                                            span,
                                        },
                                        span,
                                    })
                                    .collect(),
                            },
                            span,
                        };
                    }
                }
                point_loops(&mut for_stmt.body, sources, columns);
            }
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    point_loops(&mut branch.body, sources, columns);
                }
                if let Some(body) = &mut if_stmt.else_body {
                    point_loops(body, sources, columns);
                }
            }
            Stmt::While(while_stmt) => point_loops(&mut while_stmt.body, sources, columns),
            Stmt::Match(match_stmt) => {
                for arm in &mut match_stmt.arms {
                    point_loops(&mut arm.body, sources, columns);
                }
            }
            Stmt::With(with) => point_loops(&mut with.body, sources, columns),
            _ => {}
        }
    }
}

/// The expressions a statement evaluates itself, outside nested blocks.
fn direct_exprs(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::Assign(assign) => vec![&assign.value],
        Stmt::Expr(expr_stmt) => vec![&expr_stmt.expr],
        Stmt::Return(ret) => ret.value.iter().collect(),
        _ => Vec::new(),
    }
}

/// Calls `visit` with each call of a bare name in `expr`, outside lambdas
/// and comprehensions.
fn visit_calls(expr: &Expr, visit: &mut dyn FnMut(&str, &[Argument])) {
    match &expr.kind {
        ExprKind::Lambda { .. } | ExprKind::Comprehension { .. } | ExprKind::Match { .. } => {
            return;
        }
        ExprKind::Call { callee, args } => {
            if let ExprKind::Name(function) = &callee.kind {
                visit(function, args);
            }
        }
        _ => {}
    }
    let mut probe = expr.clone();
    probe.for_each_child_mut(&mut |child| visit_calls(child, visit));
}

/// Replaces the read of `held` in `stmt` with a call of `generator` on the
/// hidden locals that now hold its arguments.
fn replace_held(stmt: &mut Stmt, held: &str, generator: &str, hidden: &[String]) {
    fn walk(expr: &mut Expr, held: &str, generator: &str, hidden: &[String]) {
        if matches!(&expr.kind, ExprKind::Name(name) if name == held) {
            let span = expr.span;
            *expr = Expr {
                kind: ExprKind::Call {
                    callee: Box::new(Expr {
                        kind: ExprKind::Name(generator.to_string()),
                        span,
                    }),
                    args: hidden
                        .iter()
                        .map(|name| Argument {
                            name: None,
                            value: Expr {
                                kind: ExprKind::Name(name.clone()),
                                span,
                            },
                            span,
                        })
                        .collect(),
                },
                span,
            };
            return;
        }
        expr.for_each_child_mut(&mut |child| walk(child, held, generator, hidden));
    }
    match stmt {
        Stmt::Assign(assign) => walk(&mut assign.value, held, generator, hidden),
        Stmt::Expr(expr_stmt) => walk(&mut expr_stmt.expr, held, generator, hidden),
        Stmt::Return(ret) => {
            if let Some(value) = &mut ret.value {
                walk(value, held, generator, hidden);
            }
        }
        _ => {}
    }
}
