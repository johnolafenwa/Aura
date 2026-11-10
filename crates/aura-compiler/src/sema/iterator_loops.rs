//! `for` loops over user iterators (Batch 9 step 9b).
//!
//! A `for` loop whose source implements the builtin `iter.Iterator[T]`
//! trait is spelled with the protocol's own calls. The checker learns that a
//! loop iterates a user iterator only once it knows the source's type, so it
//! records the loop's source position, the module is rewritten, and the
//! module is checked again, as for property reads. The bare form
//!
//! ```text
//! for target in source:
//!     body
//! ```
//!
//! becomes
//!
//! ```text
//! mut <iterator L:C> = source
//! while true:
//!     match own <iterator L:C>.next():
//!         case Item(target):
//!             body
//!         case _:
//!             break
//! ```
//!
//! and `for target in mut place:` calls `place.next()` directly, so the loop
//! borrows the iterator and leaves it usable afterwards. The hidden local's
//! name contains a space, so no source program can name it. Every later
//! phase, including ownership analysis and both backends, sees ordinary
//! statements.

use super::{
    BTreeMap, BTreeSet, Diagnostic, Expr, ExprKind, FunctionChecker, Item, Module, ModuleContext,
    ReceiverKind, Result, Stmt, Type,
};
use crate::ast::{
    AssertStmt, AssignStmt, AssignTarget, BindingTarget, BreakStmt, ForStmt, IfBranch, IfStmt,
    MatchArm, MatchStmt, Pattern, RestPattern, TuplePattern, VariantPattern, WhileStmt,
};
use std::cell::RefCell;

/// The module and name of the builtin iteration trait.
pub(crate) const ITER_MODULE: &str = "iter";
pub(crate) const ITERATOR_TRAIT: &str = "Iterator";
pub(crate) const ITERATOR_NEXT: &str = "next";
/// The `Iterator[T]` default method that drains an iterator into a list.
pub(crate) const ITERATOR_COLLECT: &str = "collect";
/// The builtin trait for values that turn into an iterator, and its method.
pub(crate) const INTO_ITERATOR_TRAIT: &str = "IntoIterator";
pub(crate) const INTO_ITERATOR_METHOD: &str = "into_iter";

/// The builtin types a `for` loop, a comprehension, `enumerate`, and `zip`
/// read directly. An `IntoIterator` implementation never replaces that loop.
const BUILTIN_LOOP_SOURCES: [&str; 5] = ["list", "set", "Span", "Range", "Queue"];

/// A source position: line and column of the `for` statement.
type LoopSite = (usize, usize);

#[derive(Default)]
struct LoopState {
    /// The module this check belongs to. An importer also checks the method
    /// bodies of imported classes, which carry their own module's loops.
    module: String,
    discovered: BTreeSet<LoopSite>,
    /// `for` loops over `enumerate(...)` or `zip(...)` of user iterators.
    lockstep: BTreeSet<LoopSite>,
    /// Loop and comprehension sources that implement `IntoIterator`.
    into_iter: BTreeSet<LoopSite>,
    /// Discovered loops that can advance with `__aura_advance` (see
    /// `iterator_fast.rs`).
    fast: BTreeMap<LoopSite, FastLoop>,
}

/// How a discovered loop advances without a boxed `Step`.
#[derive(Clone)]
enum FastLoop {
    /// The source's class has an `__aura_advance` method.
    Advance,
    /// The source calls a simple generator, whose frame builder (named
    /// here) makes the iterator instead.
    Frame(String),
}

thread_local! {
    static LOOP_STATE: RefCell<Vec<LoopState>> = const { RefCell::new(Vec::new()) };
}

fn site(span: crate::diag::Span) -> LoopSite {
    (span.line, span.column)
}

/// The name of the hidden local that owns a consumed iterator.
fn hidden_iterator_name(span: crate::diag::Span) -> String {
    format!("iterator {}:{}", span.line, span.column)
}

/// Checks a module, rewriting `for` loops over user iterators until the
/// check discovers no further such loops.
pub(super) fn check_with_iterator_loops(
    module: Module,
    context: ModuleContext,
    check: impl Fn(Module, ModuleContext) -> Result<super::Program>,
) -> Result<super::Program> {
    let module_name = if context.module_name.is_empty() {
        "<main>".to_string()
    } else {
        context.module_name.clone()
    };
    let mut module = module;
    loop {
        LOOP_STATE.with(|state| {
            state.borrow_mut().push(LoopState {
                module: module_name.clone(),
                discovered: BTreeSet::new(),
                lockstep: BTreeSet::new(),
                into_iter: BTreeSet::new(),
                fast: BTreeMap::new(),
            })
        });
        struct Pop;
        impl Drop for Pop {
            fn drop(&mut self) {
                LOOP_STATE.with(|state| {
                    state.borrow_mut().pop();
                });
            }
        }
        let pop = Pop;
        let result = check(module.clone(), context.clone());
        let (discovered, lockstep, into_iter, fast) = LOOP_STATE.with(|state| {
            state
                .borrow_mut()
                .last_mut()
                .map(|state| {
                    (
                        std::mem::take(&mut state.discovered),
                        std::mem::take(&mut state.lockstep),
                        std::mem::take(&mut state.into_iter),
                        std::mem::take(&mut state.fast),
                    )
                })
                .unwrap_or_default()
        });
        drop(pop);
        // `IntoIterator` sources become `into_iter()` calls first; the next
        // check then finds ordinary iterator loops.
        if !into_iter.is_empty() && rewrite_into_iter_sources(&mut module, &into_iter) > 0 {
            continue;
        }
        if (discovered.is_empty() && lockstep.is_empty())
            || rewrite_module(&mut module, &discovered, &lockstep, &fast) == 0
        {
            return result;
        }
    }
}

/// Records that the comprehension source at `span` iterates a user
/// iterator, so the rewrite collects it first.
pub(super) fn discover_comprehension_source(module: &str, span: crate::diag::Span) {
    discover(module, span);
}

/// Records that the loop at `span` iterates a user iterator.
fn discover(module: &str, span: crate::diag::Span) {
    LOOP_STATE.with(|state| {
        if let Some(state) = state.borrow_mut().last_mut() {
            if state.module == module {
                state.discovered.insert(site(span));
            }
        }
    });
}

/// Records that the `for` loop at `span` iterates `enumerate(...)` or
/// `zip(...)` of user iterators, which the rewrite spells lazily.
pub(super) fn discover_lockstep_loop(module: &str, span: crate::diag::Span) {
    LOOP_STATE.with(|state| {
        if let Some(state) = state.borrow_mut().last_mut() {
            if state.module == module {
                state.lockstep.insert(site(span));
            }
        }
    });
}

impl FunctionChecker<'_> {
    /// The item type `T` when `ty` implements the builtin `iter.Iterator[T]`.
    pub(super) fn iterator_item_type(&self, ty: &Type) -> Option<Type> {
        // A type parameter bounded by `Iterator[T]` iterates its `T`.
        if let Type::TypeParam(name) = ty {
            return self.type_param_bounds.get(name)?.iter().find_map(|bound| {
                let short = bound.trait_name.rsplit(['.', ':']).next();
                (short == Some(ITERATOR_TRAIT) && bound.trait_args.len() == 1)
                    .then(|| bound.trait_args[0].clone())
            });
        }
        if !matches!(ty, Type::Named(..)) {
            return None;
        }
        self.trait_impls_in_scope().find_map(|trait_impl| {
            let info = self.trait_info_for_impl(trait_impl)?;
            if info.module_name != ITER_MODULE || info.decl.name != ITERATOR_TRAIT {
                return None;
            }
            let substitutions = self.trait_impl_substitutions(trait_impl, ty)?;
            let item = trait_impl.trait_args.first()?;
            Some(super::types::substitute_type(item, &substitutions))
        })
    }

    /// Whether `ty` implements the builtin `iter.IntoIterator[T]`, so a loop
    /// over it iterates `ty.into_iter()`.
    pub(super) fn implements_into_iterator(&self, ty: &Type) -> bool {
        // A builtin collection keeps its own loop, which reads the items in
        // place. `list[T]` implements `IntoIterator[T]` for generic code
        // only, and that implementation consumes the list.
        if matches!(ty, Type::Named(name, _) if BUILTIN_LOOP_SOURCES.contains(&name.as_str())) {
            return false;
        }
        // A type parameter bounded by `IntoIterator[T]` (with its hidden
        // associated `Iter` argument) iterates its `into_iter()`.
        if let Type::TypeParam(name) = ty {
            return self.type_param_bounds.get(name).is_some_and(|bounds| {
                bounds.iter().any(|bound| {
                    bound.trait_name.rsplit(['.', ':']).next() == Some(INTO_ITERATOR_TRAIT)
                        && bound.trait_args.len() == 2
                })
            });
        }
        self.trait_impls_in_scope().any(|trait_impl| {
            self.trait_info_for_impl(trait_impl).is_some_and(|info| {
                info.module_name == ITER_MODULE && info.decl.name == INTO_ITERATOR_TRAIT
            }) && self.trait_impl_substitutions(trait_impl, ty).is_some()
        })
    }

    /// Records that the source at `span` is an `IntoIterator` value, so the
    /// rewrite iterates `source.into_iter()`, and stops this check: the
    /// rewritten module is checked again.
    pub(super) fn discover_into_iterator_source(
        &self,
        span: crate::diag::Span,
        ty: &Type,
    ) -> Diagnostic {
        LOOP_STATE.with(|state| {
            if let Some(state) = state.borrow_mut().last_mut() {
                if state.module == self.module_name {
                    state.into_iter.insert(site(span));
                }
            }
        });
        Diagnostic::coded_at(
            "AU2002",
            span,
            format!("`{ty}` is iterated through `into_iter()`"),
        )
    }

    /// Checks the header of a `for` loop over a user iterator and records it
    /// for rewriting. Returns the loop target's type.
    pub(super) fn discover_iterator_loop(
        &self,
        for_stmt: &ForStmt,
        iterable_ty: &Type,
        item_ty: Type,
        locals: &mut std::collections::HashMap<String, super::LocalBinding>,
    ) -> Result<Type> {
        match for_stmt.borrow_mode {
            Some(ReceiverKind::Value) => {
                return Err(Diagnostic::coded_at(
                    "AU3004",
                    for_stmt.span,
                    "a `for` loop already consumes an iterator; write `for item in source:` to consume it, or `for item in mut source:` to borrow it",
                ));
            }
            Some(ReceiverKind::Borrow) => {
                return Err(Diagnostic::coded_at(
                    "AU3004",
                    for_stmt.span,
                    "a `for` loop advances its iterator, so it cannot iterate a shared borrow; write `for item in source:` to consume it, or `for item in mut source:` to borrow it",
                ));
            }
            Some(ReceiverKind::BorrowMut)
                if !self.is_mutable_place(&for_stmt.iterable, locals)? =>
            {
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    for_stmt.iterable.span,
                    "`for value in mut ...:` requires a mutable iterator place",
                ));
            }
            Some(ReceiverKind::BorrowMut) | None => {}
        }
        discover(self.module_name, for_stmt.span);
        if let Some(fast) = self.fast_loop(for_stmt, iterable_ty, locals) {
            LOOP_STATE.with(|state| {
                if let Some(state) = state.borrow_mut().last_mut() {
                    if state.module == self.module_name {
                        state.fast.insert(site(for_stmt.span), fast);
                    }
                }
            });
        }
        Ok(item_ty)
    }

    /// How the loop can advance without a boxed `Step`, if it can.
    fn fast_loop(
        &self,
        for_stmt: &ForStmt,
        iterable_ty: &Type,
        locals: &std::collections::HashMap<String, super::LocalBinding>,
    ) -> Option<FastLoop> {
        if !matches!(for_stmt.target, BindingTarget::Name { .. }) {
            return None;
        }
        if for_stmt.borrow_mode.is_none() {
            if let ExprKind::Call { callee, .. } = &for_stmt.iterable.kind {
                if let ExprKind::Name(function) = &callee.kind {
                    let builder = format!("{}{function}", super::iterator_fast::FRAME_NEW_PREFIX);
                    if !locals.contains_key(function) && self.functions.contains_key(&builder) {
                        return Some(FastLoop::Frame(builder));
                    }
                }
            }
        }
        let Type::Named(class, args) = iterable_ty else {
            return None;
        };
        self.classes
            .get(class)
            .filter(|info| {
                args.is_empty() && info.methods.contains_key(super::iterator_fast::ADVANCE)
            })
            .map(|_| FastLoop::Advance)
    }
}

/// Rewrites each recorded `IntoIterator` source into `source.into_iter()`.
fn rewrite_into_iter_sources(module: &mut Module, into_iter: &BTreeSet<LoopSite>) -> usize {
    let none = BTreeSet::new();
    rewrite_module_sources(module, &none, &none, into_iter, &BTreeMap::new())
}

fn rewrite_module(
    module: &mut Module,
    sites: &BTreeSet<LoopSite>,
    lockstep: &BTreeSet<LoopSite>,
    fast: &BTreeMap<LoopSite, FastLoop>,
) -> usize {
    rewrite_module_sources(module, sites, lockstep, &BTreeSet::new(), fast)
}

fn rewrite_module_sources(
    module: &mut Module,
    sites: &BTreeSet<LoopSite>,
    lockstep: &BTreeSet<LoopSite>,
    into_iter: &BTreeSet<LoopSite>,
    fast: &BTreeMap<LoopSite, FastLoop>,
) -> usize {
    let mut rewriter = Rewriter {
        sites,
        lockstep,
        into_iter,
        fast,
        count: 0,
    };
    for item in &mut module.items {
        match item {
            Item::Function(function) => rewriter.block(&mut function.body),
            Item::Class(class) => {
                for method in &mut class.methods {
                    rewriter.block(&mut method.body);
                }
            }
            Item::Trait(trait_decl) => {
                for method in &mut trait_decl.methods {
                    rewriter.block(&mut method.body);
                }
            }
            Item::Impl(impl_decl) => {
                for method in &mut impl_decl.methods {
                    rewriter.block(&mut method.body);
                }
            }
            Item::TypeAlias(_)
            | Item::Enum(_)
            | Item::ExternFunction(_)
            | Item::ExternOpaqueClass(_) => {}
        }
    }
    rewriter.block(&mut module.top_level_stmts);
    for constant in &mut module.constants {
        rewriter.expr(&mut constant.value);
    }
    rewriter.count
}

struct Rewriter<'a> {
    sites: &'a BTreeSet<LoopSite>,
    lockstep: &'a BTreeSet<LoopSite>,
    into_iter: &'a BTreeSet<LoopSite>,
    fast: &'a BTreeMap<LoopSite, FastLoop>,
    count: usize,
}

impl Rewriter<'_> {
    fn block(&mut self, body: &mut Vec<Stmt>) {
        let mut index = 0;
        while index < body.len() {
            self.stmt_exprs(&mut body[index]);
            self.nested(&mut body[index]);
            if let Stmt::For(for_stmt) = &mut body[index] {
                self.wrap_into_iter_source(&mut for_stmt.iterable);
            }
            let rewrite = match &body[index] {
                Stmt::For(for_stmt) if self.lockstep.contains(&site(for_stmt.span)) => Some(true),
                Stmt::For(for_stmt) if self.sites.contains(&site(for_stmt.span)) => Some(false),
                _ => None,
            };
            let Some(lockstep) = rewrite else {
                index += 1;
                continue;
            };
            let Stmt::For(for_stmt) = body.remove(index) else {
                unreachable!("the statement was just matched as a `for` loop");
            };
            let replacement = if lockstep {
                lower_lockstep_iterator_loop(for_stmt)
            } else if let Some(fast) = self.fast.get(&site(for_stmt.span)) {
                lower_fast_loop(for_stmt, fast)
            } else {
                lower_iterator_loop(for_stmt)
            };
            let added = replacement.len();
            for (offset, stmt) in replacement.into_iter().enumerate() {
                body.insert(index + offset, stmt);
            }
            index += added;
            self.count += 1;
        }
    }

    /// Rewrites the comprehension sources in `expr` that iterate a user
    /// iterator into `source.collect()`. A comprehension builds its whole
    /// result at once, so collecting the source first keeps its meaning.
    fn expr(&mut self, expr: &mut Expr) {
        if let ExprKind::Comprehension { clauses, .. } = &mut expr.kind {
            for clause in clauses {
                self.wrap_into_iter_source(&mut clause.iterable);
                self.source(&mut clause.iterable);
            }
        }
        expr.for_each_child_mut(&mut |child| self.expr(child));
    }

    /// Rewrites a loop or comprehension source, or each operand of an
    /// `enumerate(...)` or `zip(...)` source, that was recorded as an
    /// `IntoIterator` value into `source.into_iter()`.
    fn wrap_into_iter_source(&mut self, iterable: &mut Expr) {
        if let ExprKind::Call { callee, args } = &mut iterable.kind {
            if matches!(&callee.kind, ExprKind::Name(name) if name == "enumerate" || name == "zip")
            {
                for arg in args {
                    self.wrap_into_iter_source(&mut arg.value);
                }
                return;
            }
        }
        if !self.into_iter.contains(&site(iterable.span)) {
            return;
        }
        let span = iterable.span;
        let source = std::mem::replace(
            iterable,
            Expr {
                kind: ExprKind::Bool(false),
                span,
            },
        );
        *iterable = method_call(source, INTO_ITERATOR_METHOD, span);
        self.count += 1;
    }

    /// Rewrites one comprehension source, or each operand of an
    /// `enumerate(...)` or `zip(...)` source, found at a recorded site.
    fn source(&mut self, iterable: &mut Expr) {
        if let ExprKind::Call { callee, args } = &mut iterable.kind {
            if matches!(&callee.kind, ExprKind::Name(name) if name == "enumerate" || name == "zip")
            {
                for arg in args {
                    self.source(&mut arg.value);
                }
                return;
            }
        }
        if !self.sites.contains(&site(iterable.span)) {
            return;
        }
        let span = iterable.span;
        let source = std::mem::replace(
            iterable,
            Expr {
                kind: ExprKind::Bool(false),
                span,
            },
        );
        *iterable = Expr {
            kind: ExprKind::Call {
                callee: Box::new(Expr {
                    kind: ExprKind::Member {
                        object: Box::new(source),
                        field: ITERATOR_COLLECT.to_string(),
                    },
                    span,
                }),
                args: Vec::new(),
            },
            span,
        };
        self.count += 1;
    }

    /// Visits the expressions a statement holds directly.
    fn stmt_exprs(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Assign(assign) => {
                self.expr(&mut assign.value);
                match &mut assign.target {
                    AssignTarget::Name(_) => {}
                    AssignTarget::Member { object, .. } => self.expr(object),
                    AssignTarget::Index { object, index } => {
                        self.expr(object);
                        self.expr(index);
                    }
                }
            }
            Stmt::View(view) => self.expr(&mut view.source),
            Stmt::Destructure(destructure) => self.expr(&mut destructure.value),
            Stmt::Assert(assert) => {
                self.expr(&mut assert.condition);
                if let Some(message) = &mut assert.message {
                    self.expr(message);
                }
            }
            Stmt::Return(ret) => {
                if let Some(value) = &mut ret.value {
                    self.expr(value);
                }
            }
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    self.expr(&mut branch.condition);
                }
            }
            Stmt::Match(match_stmt) => {
                self.expr(&mut match_stmt.scrutinee);
                for arm in &mut match_stmt.arms {
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard);
                    }
                }
            }
            Stmt::For(for_stmt) => self.expr(&mut for_stmt.iterable),
            Stmt::With(with) => self.expr(&mut with.value),
            Stmt::While(while_stmt) => self.expr(&mut while_stmt.condition),
            Stmt::Expr(expr_stmt) => self.expr(&mut expr_stmt.expr),
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    fn nested(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    self.block(&mut branch.body);
                }
                if let Some(body) = &mut if_stmt.else_body {
                    self.block(body);
                }
            }
            Stmt::Match(match_stmt) => {
                for arm in &mut match_stmt.arms {
                    self.block(&mut arm.body);
                }
            }
            Stmt::For(for_stmt) => self.block(&mut for_stmt.body),
            Stmt::With(with) => self.block(&mut with.body),
            Stmt::While(while_stmt) => self.block(&mut while_stmt.body),
            Stmt::Assign(_)
            | Stmt::View(_)
            | Stmt::Destructure(_)
            | Stmt::Pass(_)
            | Stmt::Assert(_)
            | Stmt::Return(_)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::Expr(_) => {}
        }
    }
}

fn target_pattern(target: BindingTarget) -> Pattern {
    match target {
        BindingTarget::Name { name, span } if name == "_" => Pattern::Wildcard(span),
        BindingTarget::Name { name, span } => {
            Pattern::Binding(crate::ast::BindingPattern { name, span })
        }
        BindingTarget::Tuple { elements, span } => Pattern::Tuple(TuplePattern {
            elements: elements.into_iter().map(target_pattern).collect(),
            span,
        }),
        BindingTarget::Rest { name, span } => Pattern::Rest(RestPattern {
            name: (name != "_").then_some(name),
            span,
        }),
    }
}

/// A loop over a fast iterator:
///
/// ```text
/// mut <iterator> = source        # or the frame builder's call
/// while true:
///     <step> = <iterator>.__aura_advance()
///     if <step> is not None:
///         target = <step>
///         invariants; body
///     else:
///         break
/// ```
/// Column offset of the generated reads of a fast loop's step.
const FAST_STEP_COLUMN: usize = 1_000_000;

fn lower_fast_loop(for_stmt: ForStmt, fast: &FastLoop) -> Vec<Stmt> {
    let span = for_stmt.span;
    let source_span = for_stmt.iterable.span;
    let mut stmts = Vec::new();
    let mut iterable = for_stmt.iterable;
    if let FastLoop::Frame(builder) = fast {
        if let ExprKind::Call { callee, .. } = &mut iterable.kind {
            callee.kind = ExprKind::Name(builder.clone());
        }
    }
    let BindingTarget::Name {
        name: target,
        span: target_span,
    } = for_stmt.target
    else {
        unreachable!("fast loops bind a name");
    };
    let receiver = if for_stmt.borrow_mode == Some(ReceiverKind::BorrowMut) {
        iterable
    } else {
        let name = hidden_iterator_name(span);
        stmts.push(Stmt::Assign(AssignStmt {
            mutable: true,
            target: AssignTarget::Name(name.clone()),
            annotation: None,
            op: None,
            value: iterable,
            span,
        }));
        Expr {
            kind: ExprKind::Name(name),
            span: source_span,
        }
    };
    let step = format!("step {}:{}", span.line, span.column);
    // The checker keys expression facts by span, so each read of the step
    // gets a span of its own: the test sees `T | None`, the binding `T`.
    let step_read = |at: crate::diag::Span| Expr {
        kind: ExprKind::Name(step.clone()),
        span: crate::diag::Span::new(at.line, at.column + FAST_STEP_COLUMN),
    };
    let tested = step_read(source_span);
    let bound = step_read(target_span);
    let mut body = Vec::new();
    if target != "_" {
        body.push(Stmt::Assign(AssignStmt {
            mutable: false,
            target: AssignTarget::Name(target),
            annotation: None,
            op: None,
            value: bound,
            span: target_span,
        }));
    }
    body.extend(for_stmt.contracts.into_iter().map(invariant_check));
    body.extend(for_stmt.body);
    stmts.push(Stmt::While(WhileStmt {
        condition: Expr {
            kind: ExprKind::Bool(true),
            span,
        },
        contracts: Vec::new(),
        body: vec![
            Stmt::Assign(AssignStmt {
                mutable: false,
                target: AssignTarget::Name(step),
                annotation: None,
                op: None,
                value: method_call(receiver, super::iterator_fast::ADVANCE, source_span),
                span,
            }),
            Stmt::If(IfStmt {
                branches: vec![IfBranch {
                    condition: Expr {
                        kind: ExprKind::IsNone {
                            value: Box::new(tested),
                            negated: true,
                            operator_span: span,
                        },
                        span,
                    },
                    body,
                    span,
                }],
                else_body: Some(vec![Stmt::Break(BreakStmt { span })]),
                span,
            }),
        ],
        span,
    }));
    stmts
}

fn lower_iterator_loop(for_stmt: ForStmt) -> Vec<Stmt> {
    let span = for_stmt.span;
    let source_span = for_stmt.iterable.span;
    let mut stmts = Vec::new();
    let receiver = if for_stmt.borrow_mode == Some(ReceiverKind::BorrowMut) {
        for_stmt.iterable
    } else {
        let name = hidden_iterator_name(span);
        stmts.push(Stmt::Assign(AssignStmt {
            mutable: true,
            target: AssignTarget::Name(name.clone()),
            annotation: None,
            op: None,
            value: for_stmt.iterable,
            span,
        }));
        Expr {
            kind: ExprKind::Name(name),
            span: source_span,
        }
    };
    let next_call = Expr {
        kind: ExprKind::Call {
            callee: Box::new(Expr {
                kind: ExprKind::Member {
                    object: Box::new(receiver),
                    field: ITERATOR_NEXT.to_string(),
                },
                span: source_span,
            }),
            args: Vec::new(),
        },
        span: source_span,
    };
    let target_span = for_stmt.target.span();
    let item_arm = MatchArm {
        pattern: Pattern::Variant(VariantPattern {
            enum_name: None,
            variant_name: "Item".to_string(),
            subpatterns: vec![target_pattern(for_stmt.target)],
            span: target_span,
        }),
        guard: None,
        body: for_stmt
            .contracts
            .into_iter()
            .map(invariant_check)
            .chain(for_stmt.body)
            .collect(),
        span,
    };
    let end_arm = MatchArm {
        pattern: Pattern::Wildcard(span),
        guard: None,
        body: vec![Stmt::Break(BreakStmt { span })],
        span,
    };
    stmts.push(Stmt::While(WhileStmt {
        condition: Expr {
            kind: ExprKind::Bool(true),
            span,
        },
        contracts: Vec::new(),
        body: vec![Stmt::Match(MatchStmt {
            scrutinee: next_call,
            capability: ReceiverKind::Value,
            arms: vec![item_arm, end_arm],
            span,
        })],
        span,
    }));
    stmts
}

/// The check a `for` loop's `invariant` clause becomes at the start of the
/// rewritten loop's item arm, after the target is bound.
fn invariant_check(clause: crate::ast::ContractClause) -> Stmt {
    Stmt::Assert(AssertStmt {
        condition: clause.condition.clone(),
        message: None,
        span: clause.condition.span,
        clause: Some(Box::new(clause)),
    })
}

/// `receiver.next()` at `span`.
fn next_call(receiver: Expr, span: crate::diag::Span) -> Expr {
    method_call(receiver, ITERATOR_NEXT, span)
}

fn name_expr(name: &str, span: crate::diag::Span) -> Expr {
    Expr {
        kind: ExprKind::Name(name.to_string()),
        span,
    }
}

/// `match <scrutinee>: case <pattern>: body`, with `case _: break` when the
/// scrutinee is a `Step[T]`.
fn match_one(
    scrutinee: Expr,
    pattern: Pattern,
    body: Vec<Stmt>,
    step: bool,
    span: crate::diag::Span,
) -> Stmt {
    let pattern_span = pattern.span();
    let pattern = if step {
        Pattern::Variant(VariantPattern {
            enum_name: None,
            variant_name: "Item".to_string(),
            subpatterns: vec![pattern],
            span: pattern_span,
        })
    } else {
        pattern
    };
    let mut arms = vec![MatchArm {
        pattern,
        guard: None,
        body,
        span,
    }];
    if step {
        arms.push(MatchArm {
            pattern: Pattern::Wildcard(span),
            guard: None,
            body: vec![Stmt::Break(BreakStmt { span })],
            span,
        });
    }
    Stmt::Match(MatchStmt {
        scrutinee,
        capability: ReceiverKind::Value,
        arms,
        span,
    })
}

/// Lowers `for target in enumerate(it[, start]):` and
/// `for target in zip(a, b):` over user iterators lazily:
///
/// ```text
/// mut <index L:C>: int64 = start
/// mut <iterator L:C 0> = a
/// mut <iterator L:C 1> = b
/// while true:
///     match own <iterator L:C 0>.next():
///         case Item(x):
///             match own <iterator L:C 1>.next():
///                 case Item(y):
///                     match <index L:C>:
///                         case i:
///                             <index L:C> += 1
///                             body
///                 case _:
///                     break
///         case _:
///             break
/// ```
///
/// Each target element binds as a pattern, so it shadows an outer name the
/// way a `for` target does. A target that is not a tuple of one element per
/// position binds the whole tuple instead.
fn lower_lockstep_iterator_loop(for_stmt: ForStmt) -> Vec<Stmt> {
    let span = for_stmt.span;
    let source_span = for_stmt.iterable.span;
    let ExprKind::Call { callee, args } = for_stmt.iterable.kind else {
        unreachable!("a lockstep loop iterates an `enumerate` or `zip` call");
    };
    let enumerate = matches!(&callee.kind, ExprKind::Name(name) if name == "enumerate");
    let mut operands: Vec<Expr> = args.into_iter().map(|argument| argument.value).collect();
    let start = (enumerate && operands.len() == 2).then(|| operands.pop().expect("two arguments"));
    let site_name = format!("{}:{}", span.line, span.column);
    let index_name = format!("index {site_name}");
    let mut stmts = Vec::new();
    if enumerate {
        stmts.push(Stmt::Assign(AssignStmt {
            mutable: true,
            target: AssignTarget::Name(index_name.clone()),
            annotation: Some(crate::ast::TypeRef {
                kind: crate::ast::TypeRefKind::Named {
                    name: "int64".to_string(),
                    args: Vec::new(),
                },
                indirect: false,
                span: source_span,
            }),
            op: None,
            value: start.unwrap_or(Expr {
                kind: ExprKind::Int(0),
                span: source_span,
            }),
            span,
        }));
    }
    let iterator_names: Vec<String> = (0..operands.len())
        .map(|position| format!("iterator {site_name} {position}"))
        .collect();
    for (name, operand) in iterator_names.iter().zip(operands) {
        stmts.push(Stmt::Assign(AssignStmt {
            mutable: true,
            target: AssignTarget::Name(name.clone()),
            annotation: None,
            op: None,
            value: operand,
            span,
        }));
    }

    let positions = iterator_names.len() + usize::from(enumerate);
    let target_span = for_stmt.target.span();
    let (index_pattern, item_patterns, whole) = match for_stmt.target {
        BindingTarget::Tuple { elements, .. }
            if elements.len() == positions
                && !elements
                    .iter()
                    .any(|element| matches!(element, BindingTarget::Rest { .. })) =>
        {
            let mut patterns = elements.into_iter().map(target_pattern);
            let index = enumerate.then(|| patterns.next().expect("an index element"));
            (index, patterns.collect::<Vec<_>>(), None)
        }
        target => {
            let items: Vec<String> = (0..iterator_names.len())
                .map(|position| format!("item {site_name} {position}"))
                .collect();
            let patterns = items
                .iter()
                .map(|name| {
                    Pattern::Binding(crate::ast::BindingPattern {
                        name: name.clone(),
                        span: target_span,
                    })
                })
                .collect();
            (None, patterns, Some((target, items)))
        }
    };

    let mut body: Vec<Stmt> = for_stmt
        .contracts
        .into_iter()
        .map(invariant_check)
        .chain(for_stmt.body)
        .collect();
    if let Some((target, items)) = whole {
        let mut elements = Vec::new();
        if enumerate {
            elements.push(name_expr(&index_name, source_span));
        }
        elements.extend(items.iter().map(|name| name_expr(name, target_span)));
        body = vec![match_one(
            Expr {
                kind: ExprKind::Tuple(elements),
                span: source_span,
            },
            target_pattern(target),
            body,
            false,
            span,
        )];
    } else if let Some(pattern) = index_pattern {
        body = vec![match_one(
            name_expr(&index_name, source_span),
            pattern,
            body,
            false,
            span,
        )];
    }
    if enumerate {
        // Advance the index before the body so `continue` keeps counting.
        let increment = Stmt::Assign(AssignStmt {
            mutable: false,
            target: AssignTarget::Name(index_name.clone()),
            annotation: None,
            op: Some(crate::ast::BinaryOp::Add),
            value: Expr {
                kind: ExprKind::Int(1),
                span: source_span,
            },
            span,
        });
        let Stmt::Match(index_match) = &mut body[0] else {
            unreachable!("the body was just wrapped in a match");
        };
        index_match.arms[0].body.insert(0, increment);
    }
    for (name, pattern) in iterator_names.iter().zip(item_patterns).rev() {
        body = vec![match_one(
            next_call(name_expr(name, source_span), source_span),
            pattern,
            body,
            true,
            span,
        )];
    }
    stmts.push(Stmt::While(WhileStmt {
        condition: Expr {
            kind: ExprKind::Bool(true),
            span,
        },
        contracts: Vec::new(),
        body,
        span,
    }));
    stmts
}

/// `receiver.method()`.
fn method_call(receiver: Expr, method: &str, span: crate::diag::Span) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(Expr {
                kind: ExprKind::Member {
                    object: Box::new(receiver),
                    field: method.to_string(),
                },
                span,
            }),
            args: Vec::new(),
        },
        span,
    }
}
