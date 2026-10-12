//! Read-only computed properties (ADR-0055).
//!
//! A property read `value.name` evaluates `value` once and calls the getter
//! once with shared access. The checker learns that a member read names a
//! property only once it knows the object's type. It records the read's
//! source position, the module is rewritten so the read becomes the getter
//! call `value.name()`, and the module is checked again. Every later phase,
//! including ownership analysis and both backends, then sees an ordinary
//! shared method call. The recorded positions stay on the checked program so
//! the checker can tell a rewritten read from call syntax, which is rejected.

use super::{
    BTreeMap, BTreeSet, ClassInfo, Diagnostic, Expr, ExprKind, FunctionChecker, Item, MethodInfo,
    Module, ModuleContext, ReceiverKind, Result, Stmt, Type,
};
use std::cell::RefCell;

/// A source position: line and column of the member name.
pub(crate) type PropertySite = (usize, usize);

#[derive(Default)]
struct PropertyState {
    /// The module this check belongs to. An importer also checks the method
    /// bodies of imported classes, which carry their own module's sites.
    module: String,
    /// Reads already rewritten into getter calls.
    reads: BTreeSet<PropertySite>,
    /// Unrewritten reads the current check discovered.
    discovered: BTreeSet<PropertySite>,
}

thread_local! {
    static PROPERTY_STATE: RefCell<Vec<PropertyState>> = const { RefCell::new(Vec::new()) };
    /// The rewritten reads of every module checked on this thread, so an
    /// importer that checks an imported class body recognizes its getter calls.
    static MODULE_PROPERTY_READS: RefCell<std::collections::HashMap<String, BTreeSet<PropertySite>>> =
        RefCell::new(std::collections::HashMap::new());
}

fn with_property_state<T>(
    module: &str,
    reads: BTreeSet<PropertySite>,
    check: impl FnOnce() -> T,
) -> (T, BTreeSet<PropertySite>) {
    PROPERTY_STATE.with(|state| {
        state.borrow_mut().push(PropertyState {
            module: module.to_string(),
            reads,
            discovered: BTreeSet::new(),
        })
    });
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            PROPERTY_STATE.with(|state| {
                state.borrow_mut().pop();
            });
        }
    }
    let pop = Pop;
    let result = check();
    let discovered = PROPERTY_STATE.with(|state| {
        state
            .borrow_mut()
            .last_mut()
            .map(|state| std::mem::take(&mut state.discovered))
            .unwrap_or_default()
    });
    drop(pop);
    (result, discovered)
}

fn site(span: crate::diag::Span) -> PropertySite {
    (span.line, span.column)
}

/// Checks a module, rewriting property reads into getter calls until the
/// check discovers no further reads.
pub(super) fn check_with_property_reads(
    module: Module,
    context: ModuleContext,
    check: impl Fn(Module, ModuleContext) -> Result<super::Program>,
) -> Result<super::Program> {
    let module_name = if context.module_name.is_empty() {
        "<main>".to_string()
    } else {
        context.module_name.clone()
    };
    if !module_may_read_properties(&module, &context) {
        MODULE_PROPERTY_READS.with(|reads| reads.borrow_mut().remove(&module_name));
        return check(module, context);
    }
    let mut module = module;
    let mut reads = BTreeSet::new();
    loop {
        let (result, discovered) = with_property_state(&module_name, reads.clone(), || {
            check(module.clone(), context.clone())
        });
        let pending = discovered
            .difference(&reads)
            .copied()
            .collect::<BTreeSet<_>>();
        let rewritten = if pending.is_empty() {
            BTreeSet::new()
        } else {
            rewrite_module(&mut module, &pending)
        };
        if rewritten.is_empty() {
            MODULE_PROPERTY_READS.with(|module_reads| {
                module_reads
                    .borrow_mut()
                    .insert(module_name.clone(), reads.clone())
            });
            return result.map(|mut program| {
                program.property_reads = reads;
                program
            });
        }
        reads.extend(rewritten);
    }
}

fn module_may_read_properties(module: &Module, context: &ModuleContext) -> bool {
    let local = module.items.iter().any(|item| match item {
        Item::Class(class) => class.methods.iter().any(|method| method.property.is_some()),
        _ => false,
    });
    let has_property = |class: &ClassInfo| {
        class
            .methods
            .values()
            .any(|method| method.decl.property.is_some())
    };
    local
        || context.module_registry.values().any(|namespace| {
            namespace.classes.values().any(has_property)
                || namespace.all_classes.values().any(has_property)
        })
        || context.imported_bindings.values().any(|binding| {
            matches!(binding, super::ImportedBinding::Class(class) if has_property(class))
        })
}

pub(super) fn rewrite_module(
    module: &mut Module,
    sites: &BTreeSet<PropertySite>,
) -> BTreeSet<PropertySite> {
    let mut rewritten = BTreeSet::new();
    let mut rewriter = Rewriter {
        sites,
        rewritten: &mut rewritten,
    };
    for constant in &mut module.constants {
        rewriter.expr(&mut constant.value, false);
    }
    for item in &mut module.items {
        match item {
            Item::Function(function) => rewriter.function(function),
            Item::Class(class) => {
                for field in &mut class.fields {
                    if let Some(default) = &mut field.default {
                        rewriter.expr(default, false);
                    }
                }
                for method in &mut class.methods {
                    rewriter.function(method);
                }
            }
            Item::Trait(trait_decl) => {
                for method in &mut trait_decl.methods {
                    rewriter.function(method);
                }
            }
            Item::Impl(impl_decl) => {
                for method in &mut impl_decl.methods {
                    rewriter.function(method);
                }
            }
            Item::TypeAlias(_)
            | Item::Enum(_)
            | Item::ExternFunction(_)
            | Item::ExternOpaqueClass(_) => {}
        }
    }
    rewriter.block(&mut module.top_level_stmts);
    rewritten
}

struct Rewriter<'a> {
    sites: &'a BTreeSet<PropertySite>,
    rewritten: &'a mut BTreeSet<PropertySite>,
}

impl Rewriter<'_> {
    fn function(&mut self, function: &mut crate::ast::FunctionDecl) {
        for param in &mut function.params {
            if let Some(default) = &mut param.default {
                self.expr(default, false);
            }
        }
        self.block(&mut function.body);
    }

    fn block(&mut self, body: &mut [Stmt]) {
        for stmt in body {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Assign(assign) => {
                match &mut assign.target {
                    crate::ast::AssignTarget::Name(_) => {}
                    crate::ast::AssignTarget::Member { object, .. } => self.expr(object, false),
                    crate::ast::AssignTarget::Index { object, index } => {
                        self.expr(object, false);
                        self.expr(index, false);
                    }
                }
                self.expr(&mut assign.value, false);
            }
            Stmt::View(view) => self.expr(&mut view.source, false),
            Stmt::Destructure(destructure) => self.expr(&mut destructure.value, false),
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Assert(assert) => {
                self.expr(&mut assert.condition, false);
                if let Some(message) = &mut assert.message {
                    self.expr(message, false);
                }
            }
            Stmt::Return(ret) => {
                if let Some(value) = &mut ret.value {
                    self.expr(value, false);
                }
            }
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    self.expr(&mut branch.condition, false);
                    self.block(&mut branch.body);
                }
                if let Some(body) = &mut if_stmt.else_body {
                    self.block(body);
                }
            }
            Stmt::Match(match_stmt) => {
                self.expr(&mut match_stmt.scrutinee, false);
                for arm in &mut match_stmt.arms {
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard, false);
                    }
                    self.block(&mut arm.body);
                }
            }
            Stmt::For(for_stmt) => {
                self.expr(&mut for_stmt.iterable, false);
                self.block(&mut for_stmt.body);
            }
            Stmt::With(with) => {
                self.expr(&mut with.value, false);
                self.block(&mut with.body);
            }
            Stmt::While(while_stmt) => {
                self.expr(&mut while_stmt.condition, false);
                self.block(&mut while_stmt.body);
            }
            Stmt::Expr(expr) => self.expr(&mut expr.expr, false),
        }
    }

    /// Rewrites the reads inside `expr`. A member in callee position is the
    /// spelling `value.name(...)`, which stays a call so the checker can
    /// reject it.
    fn expr(&mut self, expr: &mut Expr, callee: bool) {
        match &mut expr.kind {
            ExprKind::Name(_)
            | ExprKind::Int(_)
            | ExprKind::DurationNanos(_)
            | ExprKind::BuiltinOmitted
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_) => {}
            ExprKind::FString(parts) => {
                for part in parts {
                    match part {
                        crate::ast::FormatPart::Literal(_) => {}
                        crate::ast::FormatPart::Expr(inner)
                        | crate::ast::FormatPart::Formatted { expr: inner, .. } => {
                            self.expr(inner, false)
                        }
                    }
                }
            }
            ExprKind::Tuple(items) | ExprKind::List(items) | ExprKind::Set(items) => {
                for item in items {
                    self.expr(item, false);
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.expr(&mut entry.key, false);
                    self.expr(&mut entry.value, false);
                }
            }
            ExprKind::Comprehension { output, clauses } => {
                match output {
                    crate::ast::ComprehensionOutput::List(value)
                    | crate::ast::ComprehensionOutput::Set(value) => self.expr(value, false),
                    crate::ast::ComprehensionOutput::Map { key, value } => {
                        self.expr(key, false);
                        self.expr(value, false);
                    }
                }
                for clause in clauses {
                    self.expr(&mut clause.iterable, false);
                    for filter in &mut clause.filters {
                        self.expr(filter, false);
                    }
                }
            }
            ExprKind::Unary { expr: inner, .. }
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Try(inner)
            | ExprKind::Group(inner) => self.expr(inner, false),
            ExprKind::Specialize { expr: inner, .. } => self.expr(inner, callee),
            ExprKind::Binary { left, right, .. } => {
                self.expr(left, false);
                self.expr(right, false);
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.expr(then_expr, false);
                self.expr(condition, false);
                self.expr(else_expr, false);
            }
            ExprKind::Lambda { body, .. } => self.expr(body, false),
            ExprKind::Call {
                callee: target,
                args,
            } => {
                self.expr(target, true);
                for arg in args {
                    self.expr(&mut arg.value, false);
                }
            }
            ExprKind::Member { object, .. } => self.expr(object, false),
            ExprKind::Index { object, index } => {
                self.expr(object, false);
                self.expr(index, false);
            }
            ExprKind::Slice {
                object, start, end, ..
            } => {
                self.expr(object, false);
                if let Some(start) = start {
                    self.expr(start, false);
                }
                if let Some(end) = end {
                    self.expr(end, false);
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, false);
                for arm in arms {
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard, false);
                    }
                    self.expr(&mut arm.value, false);
                }
            }
            ExprKind::Membership {
                value, container, ..
            } => {
                self.expr(value, false);
                self.expr(container, false);
            }
            ExprKind::CompareChain { first, links } => {
                self.expr(first, false);
                for link in links {
                    self.expr(&mut link.operand, false);
                }
            }
            ExprKind::IsNone { value, .. } => self.expr(value, false),
        }
        if callee || !matches!(expr.kind, ExprKind::Member { .. }) {
            return;
        }
        let position = site(expr.span);
        if !self.sites.contains(&position) {
            return;
        }
        let span = expr.span;
        let member = std::mem::replace(&mut expr.kind, ExprKind::BuiltinOmitted);
        expr.kind = ExprKind::Call {
            callee: Box::new(Expr { kind: member, span }),
            args: Vec::new(),
        };
        self.rewritten.insert(position);
    }
}

/// A resolved property getter on a class value.
pub(super) struct PropertyGetter<'a> {
    pub(super) class: &'a ClassInfo,
    pub(super) method: &'a MethodInfo,
    pub(super) result: Type,
}

impl FunctionChecker<'_> {
    /// The property `field` on a value of `object_ty`, if it names one.
    pub(super) fn property_getter(
        &self,
        object_ty: &Type,
        field: &str,
    ) -> Option<PropertyGetter<'_>> {
        let Type::Named(name, args) = object_ty else {
            return None;
        };
        let class = self.resolve_class_info(name)?;
        let method = class.methods.get(field)?;
        method.decl.property?;
        let substitutions = super::substitutions_from_decl_type_args(&class.decl.type_params, args);
        let result = super::substitute_type(&method.signature.return_type, &substitutions);
        Some(PropertyGetter {
            class,
            method,
            result,
        })
    }

    /// Records an unrewritten property read and returns its result type.
    pub(super) fn property_read_type(
        &self,
        object_ty: &Type,
        field: &str,
        span: crate::diag::Span,
    ) -> Result<Option<Type>> {
        let Some(getter) = self.property_getter(object_ty, field) else {
            return Ok(None);
        };
        if self.is_external_module(&getter.class.module_name) && !getter.method.decl.public {
            return Err(Diagnostic::at(
                span,
                format!(
                    "property `{}` is private on `{}`",
                    field, getter.class.decl.name
                ),
            ));
        }
        PROPERTY_STATE.with(|state| {
            if let Some(state) = state.borrow_mut().last_mut() {
                if state.module == self.module_name {
                    state.discovered.insert(site(span));
                }
            }
        });
        Ok(Some(getter.result))
    }

    /// Whether the member call at `span` is a property read the checker
    /// already rewrote into its getter call.
    pub(super) fn is_rewritten_property_read(&self, span: crate::diag::Span) -> bool {
        let position = site(span);
        let current = PROPERTY_STATE.with(|state| {
            state
                .borrow()
                .last()
                .filter(|state| state.module == self.module_name)
                .map(|state| state.reads.contains(&position))
        });
        current.unwrap_or_else(|| {
            MODULE_PROPERTY_READS.with(|reads| {
                reads
                    .borrow()
                    .get(self.module_name)
                    .is_some_and(|reads| reads.contains(&position))
            })
        })
    }

    /// The property `expr` reads, when `expr` is a property read in either
    /// its source or its rewritten form. Returns the property name.
    pub(super) fn property_read_name(
        &self,
        expr: &Expr,
        locals: &mut std::collections::HashMap<String, super::LocalBinding>,
    ) -> Option<String> {
        let expr = super::grouped_expr(expr);
        match &expr.kind {
            ExprKind::Call { callee, args }
                if args.is_empty() && self.is_rewritten_property_read(callee.span) =>
            {
                match &callee.kind {
                    ExprKind::Member { field, .. } => Some(field.clone()),
                    _ => None,
                }
            }
            ExprKind::Member { object, field } => {
                let object_ty = self.type_of_member_object_expr(object, locals).ok()?;
                self.property_getter(&object_ty, field)
                    .map(|_| field.clone())
            }
            _ => None,
        }
    }

    /// The first property read on the path from `expr` to its root place,
    /// such as `holder.total` in `holder.total.cents`.
    pub(super) fn property_read_on_place_path(
        &self,
        expr: &Expr,
        locals: &mut std::collections::HashMap<String, super::LocalBinding>,
    ) -> Option<(String, crate::diag::Span)> {
        let expr = super::grouped_expr(expr);
        if let Some(name) = self.property_read_name(expr, locals) {
            return Some((name, expr.span));
        }
        match &expr.kind {
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                self.property_read_on_place_path(object, locals)
            }
            _ => None,
        }
    }
}

impl FunctionChecker<'_> {
    /// Rejects a mutation, view, or move of `expr` when its place path reads a
    /// property: a property produces a fresh value, not a place.
    pub(super) fn reject_property_place(
        &self,
        expr: &Expr,
        action: &str,
        locals: &mut std::collections::HashMap<String, super::LocalBinding>,
    ) -> Result<()> {
        match self.property_read_on_place_path(expr, locals) {
            Some((property, span)) => Err(property_use_error(span, &property, action)),
            None => Ok(()),
        }
    }

    /// Rejects an assignment whose target is a property or reaches through
    /// one.
    pub(super) fn reject_property_assignment(
        &self,
        assign: &crate::ast::AssignStmt,
        locals: &mut std::collections::HashMap<String, super::LocalBinding>,
    ) -> Result<()> {
        let action = if assign.op.is_some() {
            "updated"
        } else {
            "assigned"
        };
        match &assign.target {
            crate::ast::AssignTarget::Name(_) => Ok(()),
            crate::ast::AssignTarget::Member { object, field } => {
                if let Ok(object_ty) = self.type_of_member_object_expr(object, locals) {
                    if self.property_getter(&object_ty, field).is_some() {
                        return Err(property_use_error(assign.span, field, action));
                    }
                }
                self.reject_property_place(object, "assigned through", locals)
            }
            crate::ast::AssignTarget::Index { object, .. } => {
                self.reject_property_place(object, "assigned through", locals)
            }
        }
    }
}

pub(super) fn property_use_error(
    span: crate::diag::Span,
    property: &str,
    action: &str,
) -> Diagnostic {
    Diagnostic::coded_at(
        "AU2045",
        span,
        format!("property `{property}` is read-only and cannot be {action}"),
    )
    .with_help(
        "a property computes a fresh value on each read; bind it to a local to change the copy",
    )
}

/// Validates the `@property` methods of one class.
pub(super) fn validate_class_properties(
    class: &crate::ast::ClassDecl,
    methods: &BTreeMap<String, MethodInfo>,
) -> Result<()> {
    for method in &class.methods {
        let Some(marker) = method.property else {
            continue;
        };
        let rule = if method.receiver != Some(ReceiverKind::Borrow) {
            Some("its receiver must be bare shared `self`")
        } else if !method.params.is_empty() {
            Some("it takes no parameters")
        } else if !method.type_params.is_empty() {
            Some("it takes no type parameters")
        } else if method.view_return.is_some() {
            Some("it must return an owned value, not a view")
        } else if methods
            .get(&method.name)
            .is_some_and(|info| info.signature.return_type == Type::Unit)
        {
            Some("it must return a value other than `None`")
        } else {
            None
        };
        if let Some(rule) = rule {
            return Err(Diagnostic::coded_at(
                "AU2044",
                method.span,
                format!("property `{}` is invalid: {rule}", method.name),
            )
            .with_secondary(marker, "`@property` is declared here")
            .with_help(format!(
                "declare it as `def {}(self) -> Type:`",
                method.name
            )));
        }
        if let Some(field) = class.fields.iter().find(|field| field.name == method.name) {
            return Err(Diagnostic::coded_at(
                "AU2044",
                method.span,
                format!(
                    "property `{}` collides with the field of the same name on `{}`",
                    method.name, class.name
                ),
            )
            .with_secondary(field.span, "the field is declared here"));
        }
    }
    Ok(())
}

/// Rejects `@property` outside a class body.
pub(super) fn reject_property_outside_class(
    method: &crate::ast::FunctionDecl,
    owner: &str,
) -> Result<()> {
    match method.property {
        Some(marker) => Err(Diagnostic::coded_at(
            "AU2044",
            marker,
            format!("`@property` is valid only on a class method, not on {owner}"),
        )
        .with_help("properties are not trait members; declare the getter in a class body")),
        None => Ok(()),
    }
}
