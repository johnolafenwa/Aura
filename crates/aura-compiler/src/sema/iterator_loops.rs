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
    BTreeSet, Diagnostic, Expr, ExprKind, FunctionChecker, Item, Module, ModuleContext,
    ReceiverKind, Result, Stmt, Type,
};
use crate::ast::{
    AssignStmt, AssignTarget, BindingTarget, BreakStmt, ForStmt, MatchArm, MatchStmt, Pattern,
    RestPattern, TuplePattern, VariantPattern, WhileStmt,
};
use std::cell::RefCell;

/// The module and name of the builtin iteration trait.
pub(crate) const ITER_MODULE: &str = "iter";
pub(crate) const ITERATOR_TRAIT: &str = "Iterator";
pub(crate) const ITERATOR_NEXT: &str = "next";

/// A source position: line and column of the `for` statement.
type LoopSite = (usize, usize);

#[derive(Default)]
struct LoopState {
    /// The module this check belongs to. An importer also checks the method
    /// bodies of imported classes, which carry their own module's loops.
    module: String,
    discovered: BTreeSet<LoopSite>,
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
        let discovered = LOOP_STATE.with(|state| {
            state
                .borrow_mut()
                .last_mut()
                .map(|state| std::mem::take(&mut state.discovered))
                .unwrap_or_default()
        });
        drop(pop);
        if discovered.is_empty() || rewrite_module(&mut module, &discovered) == 0 {
            return result;
        }
    }
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

impl FunctionChecker<'_> {
    /// The item type `T` when `ty` implements the builtin `iter.Iterator[T]`.
    pub(super) fn iterator_item_type(&self, ty: &Type) -> Option<Type> {
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

    /// Checks the header of a `for` loop over a user iterator and records it
    /// for rewriting. Returns the loop target's type.
    pub(super) fn discover_iterator_loop(
        &self,
        for_stmt: &ForStmt,
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
        if let Some(clause) = for_stmt.contracts.first() {
            return Err(Diagnostic::coded_at(
                "AU2048",
                clause.span,
                "loop clauses on a `for` loop over an iterator are not available yet",
            ));
        }
        discover(self.module_name, for_stmt.span);
        Ok(item_ty)
    }
}

fn rewrite_module(module: &mut Module, sites: &BTreeSet<LoopSite>) -> usize {
    let mut rewriter = Rewriter { sites, count: 0 };
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
    rewriter.count
}

struct Rewriter<'a> {
    sites: &'a BTreeSet<LoopSite>,
    count: usize,
}

impl Rewriter<'_> {
    fn block(&mut self, body: &mut Vec<Stmt>) {
        let mut index = 0;
        while index < body.len() {
            self.nested(&mut body[index]);
            let rewrite = matches!(
                &body[index],
                Stmt::For(for_stmt) if self.sites.contains(&site(for_stmt.span))
            );
            if !rewrite {
                index += 1;
                continue;
            }
            let Stmt::For(for_stmt) = body.remove(index) else {
                unreachable!("the statement was just matched as a `for` loop");
            };
            let replacement = lower_iterator_loop(for_stmt);
            let added = replacement.len();
            for (offset, stmt) in replacement.into_iter().enumerate() {
                body.insert(index + offset, stmt);
            }
            index += added;
            self.count += 1;
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
        body: for_stmt.body,
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
