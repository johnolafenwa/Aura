//! Fast iteration for loops over user iterators and simple generators.
//!
//! A `for` loop over a user iterator asks `next()` for a boxed `Step[T]`
//! on every item, and a loop over a generator switches stacks twice per
//! item. This pass gives both a word-sized alternative that generated code
//! keeps in registers. The checker uses it only when the result still
//! checks: the module is first checked with the pass applied, and checked
//! again as written if that check fails, so the rewrite never changes what
//! a program means or which diagnostics it gets.
//!
//! - An explicit `impl Iterator[T] for C` of a class `C` this module
//!   declares, with no type parameters, whose `next` returns only
//!   `Step.End` and `Step.Item(value)` and whose `T` cannot be `None`,
//!   moves the body of `next` into a method `C.__aura_advance(mut self) ->
//!   T | None` that returns `None` and `value` instead. `next` keeps its
//!   meaning as a wrapper around it.
//! - A module-level generator function whose parameters and locals are all
//!   numbers or `bool`, and whose body uses only assignments, `if`,
//!   `while`, `for` over `range`, `break`, `continue`, a bare `return`, and
//!   `yield`, also gets a frame class `__aura_frame_<name>`: one field per
//!   parameter and local, and a state number. Its `__aura_advance` method
//!   runs the body as a state machine, one state per resume point, and
//!   `__aura_frame_new_<name>(...)` builds a frame without running any of the
//!   body. The generator function itself is unchanged.
//!
//! The iterator-loop rewrite (see `iterator_loops.rs`) then spells a loop
//! over such a class, or over a direct call of such a generator, with
//! `__aura_advance()` and an `is None` test. Generated statements carry
//! lines far past any real line, so the checker's span-keyed tables never
//! confuse a copied expression with the original.

use super::{BTreeMap, BTreeSet, ImportedBinding, Item, Module, ModuleContext};
use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, BindingTarget, ClassDecl, Expr, ExprKind,
    ExprStmt, FieldDecl, FunctionDecl, IfBranch, IfStmt, ImplDecl, ParamMode, ReturnStmt, Stmt,
    TypeRef, TypeRefKind, UnaryOp, WhileStmt, YIELD_MARKER,
};
use crate::diag::{Span, GENERATED_COLUMN_STRIDE};
use aura_core::ops::ReceiverKind;

/// The method that advances a fast iterator: `T | None`, `None` at the end.
pub(crate) const ADVANCE: &str = "__aura_advance";
/// Prefix of a simple generator's frame class.
pub(crate) const FRAME_CLASS_PREFIX: &str = "__aura_frame_";
/// Prefix of the function that builds a simple generator's frame.
pub(crate) const FRAME_NEW_PREFIX: &str = "__aura_frame_new_";
const STATE: &str = "__aura_state";
/// The hidden module alias generated code uses for the builtin `iter`
/// module (shared with the generator pass).
const ITER_ALIAS: &str = "__aura_iter";
const MAX_FAST_FUNCTIONS: usize = 200;

/// The module with the fast forms added and its context, or `None` when
/// nothing in the module qualifies.
pub(super) fn expand(module: &Module, context: &ModuleContext) -> Option<(Module, ModuleContext)> {
    if context.module_name == super::iterator_loops::ITER_MODULE {
        return None;
    }
    let step_names = StepNames::of(context);
    let mut module = module.clone();
    let classes = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Class(class) if class.type_params.is_empty() => Some(class.name.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut windows = 0usize;
    let mut columns = 0usize;
    let mut advances = Vec::new();
    for item in &mut module.items {
        let Item::Impl(impl_decl) = item else {
            continue;
        };
        if windows >= MAX_FAST_FUNCTIONS {
            break;
        }
        if let Some(advance) = fast_iterator_impl(impl_decl, &classes, &step_names, &mut columns) {
            windows += 1;
            advances.push(advance);
        }
    }
    for (class_name, method) in advances {
        for item in &mut module.items {
            if let Item::Class(class) = item {
                if class.name == class_name {
                    class.methods.push(method);
                    break;
                }
            }
        }
    }
    let mut frames = Vec::new();
    let mut fusable = BTreeMap::new();
    for item in &module.items {
        let Item::Function(function) = item else {
            continue;
        };
        if windows >= MAX_FAST_FUNCTIONS {
            break;
        }
        if let Some(frame) = generator_frame(function, &mut columns) {
            windows += 1;
            frames.extend(frame);
            if let Some(found) = Fusable::of(function) {
                fusable.insert(function.name.clone(), found);
            }
        }
    }
    let mut fused = 0;
    if !fusable.is_empty() {
        for item in &mut module.items {
            let bodies: Vec<&mut FunctionDecl> = match item {
                Item::Function(function) => vec![function],
                Item::Class(class) => class.methods.iter_mut().collect(),
                Item::Impl(impl_decl) => impl_decl.methods.iter_mut().collect(),
                _ => Vec::new(),
            };
            for function in bodies {
                let bound = bound_names(function);
                let mut fuser = Fuser {
                    fusable: &fusable,
                    bound: &bound,
                    columns: &mut columns,
                    count: 0,
                };
                fuser.block(&mut function.body);
                fused += fuser.count;
            }
        }
    }
    if windows == 0 && fused == 0 {
        return None;
    }
    module.items.extend(frames);
    let mut context = context.clone();
    if !context.imported_bindings.contains_key(ITER_ALIAS) {
        let namespace = crate::builtin_sources::importable_builtin_namespace(&[
            super::iterator_loops::ITER_MODULE.to_string(),
        ])?;
        context
            .imported_bindings
            .insert(ITER_ALIAS.to_string(), ImportedBinding::Module(namespace));
    }
    Some((module, context))
}

/// The names under which this module sees the builtin `iter.Step` enum: a
/// bare imported name, or a module alias of `iter`.
struct StepNames {
    bare: BTreeSet<String>,
    modules: BTreeSet<String>,
}

impl StepNames {
    fn of(context: &ModuleContext) -> Self {
        let mut bare = BTreeSet::new();
        let mut modules = BTreeSet::new();
        for (name, binding) in &context.imported_bindings {
            match binding {
                ImportedBinding::Enum(info)
                    if info.module_name == super::iterator_loops::ITER_MODULE
                        && info.decl.name == "Step" =>
                {
                    bare.insert(name.clone());
                }
                ImportedBinding::Module(namespace)
                    if namespace.enums.contains_key("Step")
                        && namespace.traits.contains_key("IntoIterator") =>
                {
                    modules.insert(name.clone());
                }
                _ => {}
            }
        }
        Self { bare, modules }
    }

    /// Whether `expr` names `iter.Step` here.
    fn is_step(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Name(name) => self.bare.contains(name),
            ExprKind::Member { object, field } if field == "Step" => {
                matches!(&object.kind, ExprKind::Name(module) if self.modules.contains(module))
            }
            _ => false,
        }
    }

    /// `Some(None)` for `Step.End`, `Some(Some(value))` for
    /// `Step.Item(value)`, and `None` for anything else.
    fn step_value<'e>(&self, expr: &'e Expr) -> Option<Option<&'e Expr>> {
        match &expr.kind {
            ExprKind::Member { object, field } if field == "End" && self.is_step(object) => {
                Some(None)
            }
            ExprKind::Call { callee, args } => match &callee.kind {
                ExprKind::Member { object, field }
                    if field == "Item"
                        && self.is_step(object)
                        && args.len() == 1
                        && args[0].name.is_none() =>
                {
                    Some(Some(&args[0].value))
                }
                _ => None,
            },
            _ => None,
        }
    }
}

const SCALARS: &[&str] = &[
    "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "float32", "float64",
    "bool",
];

fn scalar_name(ty: &TypeRef) -> Option<&str> {
    match &ty.kind {
        TypeRefKind::Named { name, args }
            if args.is_empty() && SCALARS.contains(&name.as_str()) =>
        {
            Some(name)
        }
        _ => None,
    }
}

/// Whether no value of the written type `ty` is `None`, so `ty | None`
/// keeps the two apart: a number, `bool`, `str`, or a class this module
/// declares.
fn none_free(ty: &TypeRef, classes: &BTreeSet<String>) -> bool {
    match &ty.kind {
        TypeRefKind::Named { name, args } if args.is_empty() => {
            SCALARS.contains(&name.as_str()) || name == "str" || classes.contains(name)
        }
        _ => false,
    }
}

/// The class and `__aura_advance` method for an `impl Iterator[T] for C`
/// that qualifies, after moving the body of its `next` there and giving
/// `next` a wrapper body.
fn fast_iterator_impl(
    impl_decl: &mut ImplDecl,
    classes: &BTreeSet<String>,
    step_names: &StepNames,
    columns: &mut usize,
) -> Option<(String, FunctionDecl)> {
    let short = impl_decl.trait_name.rsplit('.').next()?;
    if short != super::iterator_loops::ITERATOR_TRAIT
        || !impl_decl.type_params.is_empty()
        || impl_decl.trait_args.len() != 1
        || impl_decl.derived
    {
        return None;
    }
    let TypeRefKind::Named {
        name: class_name,
        args,
    } = &impl_decl.for_type.kind
    else {
        return None;
    };
    if !args.is_empty() || !classes.contains(class_name) {
        return None;
    }
    let item = impl_decl.trait_args[0].clone();
    if !none_free(&item, classes) {
        return None;
    }
    let class_name = class_name.clone();
    let next = impl_decl
        .methods
        .iter_mut()
        .find(|method| method.name == super::iterator_loops::ITERATOR_NEXT)?;
    if next.receiver != Some(ReceiverKind::BorrowMut) || !next.params.is_empty() {
        return None;
    }
    let mut body = next.body.clone();
    if !rewrite_step_returns(&mut body, step_names) {
        return None;
    }
    let span = next.span;
    let mut advance = next.clone();
    advance.name = ADVANCE.to_string();
    advance.public = true;
    advance.body = body;
    advance.contracts = Vec::new();
    advance.return_type = TypeRef::union(
        vec![
            item,
            TypeRef::named("None", Vec::new(), false, next.return_type.span),
        ],
        next.return_type.span,
    );
    // `next` keeps its contracts and signature; its body asks the advance
    // method and wraps what it returns.
    next.body = step_wrapper(span);
    renumber(&mut next.body, columns);
    Some((class_name, advance))
}

/// Rewrites every `return Step.End` to `return None` and every
/// `return Step.Item(value)` to `return value` in `body`. False when some
/// `return` has another form.
fn rewrite_step_returns(body: &mut [Stmt], step_names: &StepNames) -> bool {
    body.iter_mut().all(|stmt| match stmt {
        Stmt::Return(ret) => {
            let Some(value) = &ret.value else {
                return false;
            };
            match step_names.step_value(value) {
                Some(None) => {
                    ret.value = Some(name_expr("None", value.span));
                    true
                }
                Some(Some(item)) => {
                    ret.value = Some(item.clone());
                    true
                }
                None => false,
            }
        }
        Stmt::If(if_stmt) => {
            if_stmt
                .branches
                .iter_mut()
                .all(|branch| rewrite_step_returns(&mut branch.body, step_names))
                && if_stmt
                    .else_body
                    .as_mut()
                    .is_none_or(|body| rewrite_step_returns(body, step_names))
        }
        Stmt::Match(match_stmt) => match_stmt
            .arms
            .iter_mut()
            .all(|arm| rewrite_step_returns(&mut arm.body, step_names)),
        Stmt::For(for_stmt) => rewrite_step_returns(&mut for_stmt.body, step_names),
        Stmt::While(while_stmt) => rewrite_step_returns(&mut while_stmt.body, step_names),
        Stmt::With(with) => rewrite_step_returns(&mut with.body, step_names),
        Stmt::Assign(_)
        | Stmt::View(_)
        | Stmt::Destructure(_)
        | Stmt::Pass(_)
        | Stmt::Assert(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Expr(_) => true,
    })
}

/// The body of a fast iterator's `next`:
///
/// ```text
/// item = self.__aura_advance()
/// if item is None:
///     return __aura_iter.Step.End
/// return __aura_iter.Step.Item(item)
/// ```
fn step_wrapper(span: Span) -> Vec<Stmt> {
    let item = "item advanced";
    let step = |field: &str| member(name_expr(ITER_ALIAS, span), "Step", span, field);
    vec![
        assign(
            item,
            method_call(name_expr("self", span), ADVANCE, span),
            false,
            span,
        ),
        Stmt::If(IfStmt {
            branches: vec![IfBranch {
                condition: is_none(name_expr(item, span), span),
                body: vec![ret(Some(step("End")), span)],
                span,
            }],
            else_body: None,
            span,
        }),
        ret(
            Some(call(step("Item"), vec![name_expr(item, span)], span)),
            span,
        ),
    ]
}

// ---------------------------------------------------------------------------
// Generator frames

/// The type of each parameter and local of a simple generator, in field
/// order, and the hidden counters of its `for` loops.
struct FrameLayout {
    fields: Vec<(String, String)>,
    locals: BTreeMap<String, String>,
    loops: usize,
}

impl FrameLayout {
    fn ty(&self, name: &str) -> Option<&str> {
        self.locals.get(name).map(String::as_str)
    }

    fn declare(&mut self, name: &str, ty: String) -> bool {
        match self.locals.get(name) {
            Some(existing) => *existing == ty,
            None => {
                self.locals.insert(name.to_string(), ty.clone());
                self.fields.push((name.to_string(), ty));
                true
            }
        }
    }

    /// The type of `expr` when every name in it is a parameter or local
    /// whose type is known, by the checker's own rules for these forms.
    fn infer(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Int(_) => Some("int64".to_string()),
            ExprKind::Float(_) => Some("float64".to_string()),
            ExprKind::Bool(_) => Some("bool".to_string()),
            ExprKind::Name(name) => self.ty(name).map(str::to_string),
            ExprKind::Group(inner) => self.infer(inner),
            ExprKind::Cast { ty, .. } => scalar_name(ty).map(str::to_string),
            ExprKind::Unary { op, expr } => match op {
                UnaryOp::Not => Some("bool".to_string()),
                UnaryOp::Neg | UnaryOp::BitNot => self.infer(expr),
            },
            ExprKind::Binary { op, left, right } => match op {
                BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Less
                | BinaryOp::LessEq
                | BinaryOp::Greater
                | BinaryOp::GreaterEq => Some("bool".to_string()),
                BinaryOp::Div | BinaryOp::Pow => None,
                _ => {
                    let literal = |side: &Expr| matches!(side.kind, ExprKind::Int(_));
                    match (literal(left), literal(right)) {
                        (true, false) => self.infer(right),
                        (false, true) => self.infer(left),
                        _ => {
                            let left = self.infer(left)?;
                            (self.infer(right)? == left).then_some(left)
                        }
                    }
                }
            },
            ExprKind::CompareChain { .. } | ExprKind::IsNone { .. } => Some("bool".to_string()),
            _ => None,
        }
    }
}

/// Whether `expr` may move into a frame: it binds no names of its own and
/// does nothing a field read could change.
fn frame_expr(expr: &Expr) -> bool {
    let own = !matches!(
        expr.kind,
        ExprKind::Lambda { .. }
            | ExprKind::Comprehension { .. }
            | ExprKind::Match { .. }
            | ExprKind::Try(_)
    );
    let mut children = true;
    let mut probe = expr.clone();
    probe.for_each_child_mut(&mut |child| children &= frame_expr(child));
    own && children
}

/// `range(...)` arguments `(start, end)` of a step-one `range` source.
fn range_bounds(iterable: &Expr) -> Option<(Expr, Expr)> {
    let ExprKind::Call { callee, args } = &iterable.kind else {
        return None;
    };
    if !matches!(&callee.kind, ExprKind::Name(name) if name == "range")
        || args.iter().any(|arg| arg.name.is_some())
    {
        return None;
    }
    let span = iterable.span;
    match args.as_slice() {
        [end] => Some((
            Expr {
                kind: ExprKind::Int(0),
                span,
            },
            end.value.clone(),
        )),
        [start, end] => Some((start.value.clone(), end.value.clone())),
        _ => None,
    }
}

fn yield_value(stmt: &Stmt) -> Option<&Expr> {
    let Stmt::Expr(ExprStmt { expr, .. }) = stmt else {
        return None;
    };
    let ExprKind::Call { callee, args } = &expr.kind else {
        return None;
    };
    match (&callee.kind, args.as_slice()) {
        (ExprKind::Name(name), [value]) if name == YIELD_MARKER => Some(&value.value),
        _ => None,
    }
}

/// Collects the locals of a simple generator body and their types. False
/// when the body uses anything a frame cannot hold.
fn collect_locals(
    body: &[Stmt],
    layout: &mut FrameLayout,
    loop_targets: &mut BTreeSet<String>,
) -> bool {
    body.iter().all(|stmt| {
        if let Some(value) = yield_value(stmt) {
            return frame_expr(value);
        }
        match stmt {
            Stmt::Assign(assign) => {
                let AssignTarget::Name(target) = &assign.target else {
                    return false;
                };
                if !frame_expr(&assign.value) || loop_targets.contains(target) {
                    return false;
                }
                if assign.op.is_some() {
                    return layout.ty(target).is_some();
                }
                let ty = match &assign.annotation {
                    Some(annotation) => scalar_name(annotation).map(str::to_string),
                    None => layout.infer(&assign.value),
                };
                ty.is_some_and(|ty| layout.declare(target, ty))
            }
            Stmt::Expr(expr_stmt) => frame_expr(&expr_stmt.expr),
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Return(ret) => ret.value.is_none(),
            Stmt::If(if_stmt) => {
                if_stmt.branches.iter().all(|branch| {
                    frame_expr(&branch.condition)
                        && collect_locals(&branch.body, layout, loop_targets)
                }) && if_stmt
                    .else_body
                    .as_ref()
                    .is_none_or(|body| collect_locals(body, layout, loop_targets))
            }
            Stmt::While(while_stmt) => {
                while_stmt.contracts.is_empty()
                    && frame_expr(&while_stmt.condition)
                    && collect_locals(&while_stmt.body, layout, loop_targets)
            }
            Stmt::For(for_stmt) => {
                let BindingTarget::Name { name, .. } = &for_stmt.target else {
                    return false;
                };
                let Some((start, end)) = range_bounds(&for_stmt.iterable) else {
                    return false;
                };
                let int64 = |expr: &Expr| {
                    matches!(expr.kind, ExprKind::Int(_))
                        || layout.infer(expr).as_deref() == Some("int64")
                };
                if for_stmt.borrow_mode.is_some()
                    || !for_stmt.contracts.is_empty()
                    || !frame_expr(&start)
                    || !frame_expr(&end)
                    || !int64(&start)
                    || !int64(&end)
                    || layout.ty(name).is_some()
                    || !layout.declare(name, "int64".to_string())
                {
                    return false;
                }
                layout.loops += 1;
                loop_targets.insert(name.clone());
                let ok = collect_locals(&for_stmt.body, layout, loop_targets);
                loop_targets.remove(name);
                ok
            }
            Stmt::View(_)
            | Stmt::Destructure(_)
            | Stmt::Assert(_)
            | Stmt::Match(_)
            | Stmt::With(_) => false,
        }
    })
}

/// The frame class and builder function of `function` when it is a simple
/// generator.
fn generator_frame(function: &FunctionDecl, columns: &mut usize) -> Option<Vec<Item>> {
    let TypeRefKind::Named { name, args } = &function.return_type.kind else {
        return None;
    };
    if name.rsplit('.').next() != Some("Generator") || args.len() != 1 {
        return None;
    }
    let item = args[0].clone();
    if !none_free(&item, &BTreeSet::new()) {
        return None;
    }
    if !function.type_params.is_empty()
        || !function.contracts.is_empty()
        || !function.decorators.is_empty()
        || !super::generators::body_yields(&function.body)
    {
        return None;
    }
    let mut layout = FrameLayout {
        fields: Vec::new(),
        locals: BTreeMap::new(),
        loops: 0,
    };
    for param in &function.params {
        let ty = scalar_name(&param.ty)?;
        if param.mode == ParamMode::BorrowMut || !layout.declare(&param.name, ty.to_string()) {
            return None;
        }
    }
    if !collect_locals(&function.body, &mut layout, &mut BTreeSet::new()) {
        return None;
    }
    let span = function.span;
    let fields = layout.locals.keys().cloned().collect::<BTreeSet<_>>();

    let mut machine = Machine {
        blocks: vec![Block::default()],
        current: 0,
        loops: Vec::new(),
        fields: &fields,
        counters: 0,
        span,
    };
    machine.lower(&function.body);
    machine.finish(Terminator::Done);
    let done = machine.blocks.len();

    let class_name = format!("{FRAME_CLASS_PREFIX}{}", function.name);
    let mut class_fields = layout
        .fields
        .iter()
        .map(|(name, ty)| field(name, ty, span))
        .collect::<Vec<_>>();
    class_fields.push(field(STATE, "int64", span));
    for counter in 0..machine.counters {
        class_fields.push(field(&counter_name(counter), "int64", span));
        class_fields.push(field(&end_name(counter), "int64", span));
    }

    // Each resume point gets a dispatch arm; jumps between blocks run
    // straight on where they can, so a call resumes and reaches its next
    // `yield` without going around the dispatch loop.
    let mut threader = Threader {
        blocks: &machine.blocks,
        done,
        span,
        needed: BTreeSet::from([0]),
        budget: 0,
    };
    let mut emitted = BTreeSet::new();
    let mut dispatch_branches = Vec::new();
    while let Some(state) = threader.needed.difference(&emitted).next().copied() {
        emitted.insert(state);
        threader.budget = THREAD_BUDGET;
        let body = threader.emit(state, &mut vec![state]);
        dispatch_branches.push(IfBranch {
            condition: binary(
                BinaryOp::Eq,
                self_field(STATE, span),
                int_expr(state as u128, span),
                span,
            ),
            body,
            span,
        });
    }
    let dispatch = Stmt::If(IfStmt {
        branches: dispatch_branches,
        else_body: Some(vec![ret(Some(name_expr("None", span)), span)]),
        span,
    });
    let mut advance = function.clone();
    advance.name = ADVANCE.to_string();
    advance.public = true;
    advance.receiver = Some(ReceiverKind::BorrowMut);
    advance.params = Vec::new();
    advance.doc = None;
    advance.span = generated_span(span, columns);
    advance.return_type = TypeRef::union(
        vec![
            item.clone(),
            TypeRef::named("None", Vec::new(), false, span),
        ],
        span,
    );
    advance.body = vec![
        Stmt::While(WhileStmt {
            condition: Expr {
                kind: ExprKind::Bool(true),
                span,
            },
            contracts: Vec::new(),
            body: vec![dispatch],
            span,
        }),
        ret(Some(name_expr("None", span)), span),
    ];

    renumber(&mut advance.body, columns);
    let class = ClassDecl {
        public: false,
        copy: false,
        name: class_name.clone(),
        type_params: Vec::new(),
        type_param_bounds: BTreeMap::new(),
        fields: class_fields,
        methods: vec![advance],
        span,
        doc: None,
    };

    // The builder takes the generator's parameters and starts every local
    // at zero, in state 0.
    let mut builder = function.clone();
    builder.name = format!("{FRAME_NEW_PREFIX}{}", function.name);
    builder.public = false;
    builder.span = generated_span(span, columns);
    builder.doc = None;
    builder.return_type = TypeRef::named(class_name.clone(), Vec::new(), false, span);
    for param in &mut builder.params {
        param.span = generated_span(param.span, columns);
    }
    let params = function
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect::<BTreeSet<_>>();
    let mut inits = Vec::new();
    for (name, ty) in &layout.fields {
        let value = if params.contains(name) {
            name_expr(name, span)
        } else {
            zero(ty, span)
        };
        inits.push((name.clone(), value));
    }
    inits.push((STATE.to_string(), int_expr(0, span)));
    for counter in 0..machine.counters {
        inits.push((counter_name(counter), int_expr(0, span)));
        inits.push((end_name(counter), int_expr(0, span)));
    }
    let construct = Expr {
        kind: ExprKind::Call {
            callee: Box::new(name_expr(&class_name, span)),
            args: inits
                .into_iter()
                .map(|(name, value)| Argument {
                    name: Some(name),
                    value,
                    span,
                })
                .collect(),
        },
        span,
    };
    builder.body = vec![ret(Some(construct), span)];
    renumber(&mut builder.body, columns);
    Some(vec![Item::Class(class), Item::Function(builder)])
}

fn counter_name(index: usize) -> String {
    format!("__aura_index_{index}")
}

fn end_name(index: usize) -> String {
    format!("__aura_end_{index}")
}

fn zero(ty: &str, span: Span) -> Expr {
    let kind = match ty {
        "bool" => ExprKind::Bool(false),
        "float32" | "float64" => ExprKind::Float(0.0),
        _ => ExprKind::Int(0),
    };
    Expr { kind, span }
}

fn field(name: &str, ty: &str, span: Span) -> FieldDecl {
    FieldDecl {
        public: false,
        name: name.to_string(),
        ty: TypeRef::named(ty, Vec::new(), false, span),
        default: None,
        span,
        doc: None,
    }
}

#[derive(Clone)]
enum Terminator {
    Goto(usize),
    Branch(Expr, usize, usize),
    Yield(Expr, usize),
    Done,
}

#[derive(Default)]
struct Block {
    stmts: Vec<Stmt>,
    terminator: Option<Terminator>,
}

/// Lowers a simple generator body into blocks that end at each resume
/// point and each jump.
struct Machine<'a> {
    blocks: Vec<Block>,
    current: usize,
    /// The `continue` and `break` targets of each enclosing loop.
    loops: Vec<(usize, usize)>,
    fields: &'a BTreeSet<String>,
    counters: usize,
    span: Span,
}

impl Machine<'_> {
    fn block(&mut self) -> usize {
        self.blocks.push(Block::default());
        self.blocks.len() - 1
    }

    /// Ends the current block with `terminator` and continues in a new,
    /// unreachable one.
    fn finish(&mut self, terminator: Terminator) {
        if self.blocks[self.current].terminator.is_none() {
            self.blocks[self.current].terminator = Some(terminator);
        }
    }

    fn jump_to(&mut self, terminator: Terminator, next: usize) {
        self.finish(terminator);
        self.current = next;
    }

    fn push(&mut self, stmt: Stmt) {
        if self.blocks[self.current].terminator.is_none() {
            self.blocks[self.current].stmts.push(stmt);
        }
    }

    /// `expr` with each local read through `self` and its lines moved.
    fn framed(&self, expr: &Expr) -> Expr {
        let mut expr = expr.clone();
        frame_names(&mut expr, self.fields);
        expr
    }

    fn lower(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.lower_stmt(stmt);
        }
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        let span = self.span;
        if let Some(value) = yield_value(stmt) {
            let value = self.framed(value);
            let resume = self.block();
            self.jump_to(Terminator::Yield(value, resume), resume);
            return;
        }
        match stmt {
            Stmt::Assign(assign) => {
                let AssignTarget::Name(target) = &assign.target else {
                    unreachable!("a frame body assigns names");
                };
                let value = self.framed(&assign.value);
                self.push(Stmt::Assign(AssignStmt {
                    mutable: false,
                    target: AssignTarget::Member {
                        object: Box::new(name_expr("self", span)),
                        field: target.clone(),
                    },
                    annotation: None,
                    op: assign.op,
                    value,
                    span: assign.span,
                }));
            }
            Stmt::Expr(expr_stmt) => {
                let mut framed = expr_stmt.clone();
                framed.expr = self.framed(&expr_stmt.expr);
                framed.span = expr_stmt.span;
                self.push(Stmt::Expr(framed));
            }
            Stmt::Pass(_) => {}
            Stmt::Break(_) => {
                let (_, exit) = *self.loops.last().expect("checked loops hold `break`");
                let dead = self.block();
                self.jump_to(Terminator::Goto(exit), dead);
            }
            Stmt::Continue(_) => {
                let (next, _) = *self.loops.last().expect("checked loops hold `continue`");
                let dead = self.block();
                self.jump_to(Terminator::Goto(next), dead);
            }
            Stmt::Return(_) => {
                let dead = self.block();
                self.jump_to(Terminator::Done, dead);
            }
            Stmt::If(if_stmt) => {
                let join = self.block();
                for branch in &if_stmt.branches {
                    let then_block = self.block();
                    let else_block = self.block();
                    let condition = self.framed(&branch.condition);
                    self.jump_to(
                        Terminator::Branch(condition, then_block, else_block),
                        then_block,
                    );
                    self.lower(&branch.body);
                    self.jump_to(Terminator::Goto(join), else_block);
                }
                if let Some(body) = &if_stmt.else_body {
                    self.lower(body);
                }
                self.jump_to(Terminator::Goto(join), join);
            }
            Stmt::While(while_stmt) => {
                let head = self.block();
                let body = self.block();
                let exit = self.block();
                self.jump_to(Terminator::Goto(head), head);
                let condition = self.framed(&while_stmt.condition);
                self.jump_to(Terminator::Branch(condition, body, exit), body);
                self.loops.push((head, exit));
                self.lower(&while_stmt.body);
                self.loops.pop();
                self.jump_to(Terminator::Goto(head), exit);
            }
            Stmt::For(for_stmt) => {
                let BindingTarget::Name { name, .. } = &for_stmt.target else {
                    unreachable!("a frame body loops over names");
                };
                let (start, end) =
                    range_bounds(&for_stmt.iterable).expect("a frame body loops over `range`");
                let counter = counter_name(self.counters);
                let limit = end_name(self.counters);
                self.counters += 1;
                let start = self.framed(&start);
                let end = self.framed(&end);
                self.push(set_field(&counter, start, span));
                self.push(set_field(&limit, end, span));
                let head = self.block();
                let body = self.block();
                let step = self.block();
                let exit = self.block();
                self.jump_to(Terminator::Goto(head), head);
                let condition = binary(
                    BinaryOp::Less,
                    self_field(&counter, span),
                    self_field(&limit, span),
                    span,
                );
                self.jump_to(Terminator::Branch(condition, body, exit), body);
                self.push(set_field(name, self_field(&counter, span), span));
                self.loops.push((step, exit));
                self.lower(&for_stmt.body);
                self.loops.pop();
                self.jump_to(Terminator::Goto(step), step);
                // The counter is below the limit here, so adding one cannot
                // overflow, as for the loop it replaces.
                self.push(Stmt::Assign(AssignStmt {
                    mutable: false,
                    target: AssignTarget::Member {
                        object: Box::new(name_expr("self", span)),
                        field: counter.clone(),
                    },
                    annotation: None,
                    op: Some(BinaryOp::Add),
                    value: int_expr(1, span),
                    span,
                }));
                self.jump_to(Terminator::Goto(head), exit);
            }
            Stmt::View(_)
            | Stmt::Destructure(_)
            | Stmt::Assert(_)
            | Stmt::Match(_)
            | Stmt::With(_) => unreachable!("collect_locals rejects these statements"),
        }
    }
}

/// Blocks a dispatch arm may run straight into before it sets the state
/// and goes around the dispatch loop instead.
const THREAD_BUDGET: usize = 32;

/// Emits the dispatch arms of a frame's state machine.
struct Threader<'b> {
    blocks: &'b [Block],
    done: usize,
    span: Span,
    /// States some arm sets, which need an arm of their own.
    needed: BTreeSet<usize>,
    budget: usize,
}

impl Threader<'_> {
    fn go(&mut self, state: usize) -> Stmt {
        if state != self.done {
            self.needed.insert(state);
        }
        set_field(STATE, int_expr(state as u128, self.span), self.span)
    }

    /// Block `index` and what follows it, up to the next resume point.
    fn emit(&mut self, index: usize, path: &mut Vec<usize>) -> Vec<Stmt> {
        let span = self.span;
        let block = &self.blocks[index];
        let mut out = block.stmts.clone();
        match block.terminator.clone().unwrap_or(Terminator::Done) {
            Terminator::Goto(next) => out.extend(self.target(next, path)),
            Terminator::Branch(condition, then_block, else_block) => {
                let then_body = self.target(then_block, path);
                let else_body = self.target(else_block, path);
                out.push(Stmt::If(IfStmt {
                    branches: vec![IfBranch {
                        condition,
                        body: then_body,
                        span,
                    }],
                    else_body: Some(else_body),
                    span,
                }));
            }
            Terminator::Yield(value, resume) => {
                out.push(self.go(resume));
                out.push(ret(Some(value), span));
            }
            Terminator::Done => {
                out.push(self.go(self.done));
                out.push(ret(Some(name_expr("None", span)), span));
            }
        }
        out
    }

    /// A jump to block `next`: its code, or setting the state when it is
    /// already on this path (a loop) or the arm is long enough.
    fn target(&mut self, next: usize, path: &mut Vec<usize>) -> Vec<Stmt> {
        if path.contains(&next) || self.budget == 0 {
            return vec![self.go(next)];
        }
        self.budget -= 1;
        path.push(next);
        let body = self.emit(next, path);
        path.pop();
        body
    }
}

/// Reads and writes each name in `fields` through `self`. A bare callee
/// keeps its name, since no field holds a function.
fn frame_names(expr: &mut Expr, fields: &BTreeSet<String>) {
    if let ExprKind::Name(name) = &expr.kind {
        if fields.contains(name) {
            let span = expr.span;
            *expr = member_of_self(name, span);
        }
        return;
    }
    if let ExprKind::Call { callee, args } = &mut expr.kind {
        if !matches!(callee.kind, ExprKind::Name(_)) {
            frame_names(callee, fields);
        }
        for arg in args {
            frame_names(&mut arg.value, fields);
        }
        return;
    }
    expr.for_each_child_mut(&mut |child| frame_names(child, fields));
}

/// `span` moved to a column of its own that still reads as `span` (see
/// `GENERATED_COLUMN_STRIDE`).
fn generated_span(span: Span, next: &mut usize) -> Span {
    *next += 1;
    Span::new(
        span.line,
        span.column % GENERATED_COLUMN_STRIDE + *next * GENERATED_COLUMN_STRIDE,
    )
}

/// Gives every generated statement and expression in `body` a span of its
/// own on its line. The checker keys facts about expressions by span, and
/// generated code reads the same names in many places.
fn renumber(body: &mut [Stmt], next: &mut usize) {
    fn fresh(span: &mut Span, next: &mut usize) {
        *span = generated_span(*span, next);
    }
    fn expr(value: &mut Expr, next: &mut usize) {
        fresh(&mut value.span, next);
        if let ExprKind::Call { args, .. } = &mut value.kind {
            for arg in args.iter_mut() {
                fresh(&mut arg.span, next);
            }
        }
        if let ExprKind::IsNone { operator_span, .. } = &mut value.kind {
            fresh(operator_span, next);
        }
        value.for_each_child_mut(&mut |child| expr(child, next));
    }
    for stmt in body {
        match stmt {
            Stmt::Assign(assign) => {
                fresh(&mut assign.span, next);
                if let AssignTarget::Member { object, .. } = &mut assign.target {
                    expr(object, next);
                }
                expr(&mut assign.value, next);
            }
            Stmt::Expr(expr_stmt) => {
                fresh(&mut expr_stmt.span, next);
                expr(&mut expr_stmt.expr, next);
            }
            Stmt::Return(ret) => {
                fresh(&mut ret.span, next);
                if let Some(value) = &mut ret.value {
                    expr(value, next);
                }
            }
            Stmt::If(if_stmt) => {
                fresh(&mut if_stmt.span, next);
                for branch in &mut if_stmt.branches {
                    fresh(&mut branch.span, next);
                    expr(&mut branch.condition, next);
                    renumber(&mut branch.body, next);
                }
                if let Some(body) = &mut if_stmt.else_body {
                    renumber(body, next);
                }
            }
            Stmt::While(while_stmt) => {
                fresh(&mut while_stmt.span, next);
                expr(&mut while_stmt.condition, next);
                renumber(&mut while_stmt.body, next);
            }
            Stmt::Break(stmt) => fresh(&mut stmt.span, next),
            Stmt::Continue(stmt) => fresh(&mut stmt.span, next),
            Stmt::Pass(stmt) => fresh(&mut stmt.span, next),
            Stmt::For(for_stmt) => {
                fresh(&mut for_stmt.span, next);
                expr(&mut for_stmt.iterable, next);
                renumber(&mut for_stmt.body, next);
            }
            Stmt::Match(_)
            | Stmt::With(_)
            | Stmt::View(_)
            | Stmt::Destructure(_)
            | Stmt::Assert(_) => {}
        }
    }
}

fn name_expr(name: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Name(name.to_string()),
        span,
    }
}

fn int_expr(value: u128, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Int(value),
        span,
    }
}

fn member_of_self(field: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Member {
            object: Box::new(name_expr("self", span)),
            field: field.to_string(),
        },
        span,
    }
}

fn self_field(field: &str, span: Span) -> Expr {
    member_of_self(field, span)
}

fn member(object: Expr, middle: &str, span: Span, field: &str) -> Expr {
    Expr {
        kind: ExprKind::Member {
            object: Box::new(Expr {
                kind: ExprKind::Member {
                    object: Box::new(object),
                    field: middle.to_string(),
                },
                span,
            }),
            field: field.to_string(),
        },
        span,
    }
}

fn call(callee: Expr, args: Vec<Expr>, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(callee),
            args: args
                .into_iter()
                .map(|value| Argument {
                    name: None,
                    value,
                    span,
                })
                .collect(),
        },
        span,
    }
}

fn method_call(object: Expr, method: &str, span: Span) -> Expr {
    call(
        Expr {
            kind: ExprKind::Member {
                object: Box::new(object),
                field: method.to_string(),
            },
            span,
        },
        Vec::new(),
        span,
    )
}

fn binary(op: BinaryOp, left: Expr, right: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span,
    }
}

fn is_none(value: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::IsNone {
            value: Box::new(value),
            negated: false,
            operator_span: span,
        },
        span,
    }
}

fn assign(name: &str, value: Expr, mutable: bool, span: Span) -> Stmt {
    Stmt::Assign(AssignStmt {
        mutable,
        target: AssignTarget::Name(name.to_string()),
        annotation: None,
        op: None,
        value,
        span,
    })
}

fn set_field(field: &str, value: Expr, span: Span) -> Stmt {
    Stmt::Assign(AssignStmt {
        mutable: false,
        target: AssignTarget::Member {
            object: Box::new(name_expr("self", span)),
            field: field.to_string(),
        },
        annotation: None,
        op: None,
        value,
        span,
    })
}

fn ret(value: Option<Expr>, span: Span) -> Stmt {
    Stmt::Return(ReturnStmt {
        value,
        view: None,
        span,
    })
}

// ---------------------------------------------------------------------------
// Generator fusion

/// A simple generator with one `yield`, whose body a loop
/// over it can run in place: each item runs the loop body where the
/// generator yields it.
struct Fusable {
    params: Vec<String>,
    /// Whether the generator has a `return`.
    returns: bool,
    fields: BTreeSet<String>,
    body: Vec<Stmt>,
}

impl Fusable {
    fn of(function: &FunctionDecl) -> Option<Self> {
        if function
            .params
            .iter()
            .any(|param| param.keyword_only || param.default.is_some())
        {
            return None;
        }
        let mut layout = FrameLayout {
            fields: Vec::new(),
            locals: BTreeMap::new(),
            loops: 0,
        };
        for param in &function.params {
            layout.declare(&param.name, scalar_name(&param.ty)?.to_string());
        }
        if !collect_locals(&function.body, &mut layout, &mut BTreeSet::new()) {
            return None;
        }
        let fields = layout.locals.keys().cloned().collect::<BTreeSet<_>>();
        let mut yields = 0;
        let mut closed = true;
        visit_stmts(&function.body, &mut |stmt| {
            if yield_value(stmt).is_some() {
                yields += 1;
            }
            for expr in stmt_exprs(stmt) {
                closed &= names_are_fields(expr, &fields);
            }
        });
        // A call in the body could trap, and a trap's call chain names the
        // generator only while it runs in a frame of its own.
        if callee_names(&function.body)
            .iter()
            .any(|name| name != "range")
        {
            return None;
        }
        let mut returns = false;
        visit_stmts(&function.body, &mut |stmt| {
            returns |= matches!(stmt, Stmt::Return(_))
        });
        (yields == 1 && closed).then(|| Self {
            returns,
            params: function
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect(),
            fields,
            body: function.body.clone(),
        })
    }
}

/// Calls `visit` on every statement of `body`, at any depth.
fn visit_stmts(body: &[Stmt], visit: &mut dyn FnMut(&Stmt)) {
    for stmt in body {
        visit(stmt);
        match stmt {
            Stmt::If(if_stmt) => {
                for branch in &if_stmt.branches {
                    visit_stmts(&branch.body, visit);
                }
                if let Some(body) = &if_stmt.else_body {
                    visit_stmts(body, visit);
                }
            }
            Stmt::While(while_stmt) => visit_stmts(&while_stmt.body, visit),
            Stmt::For(for_stmt) => visit_stmts(&for_stmt.body, visit),
            Stmt::Match(match_stmt) => {
                for arm in &match_stmt.arms {
                    visit_stmts(&arm.body, visit);
                }
            }
            Stmt::With(with) => visit_stmts(&with.body, visit),
            _ => {}
        }
    }
}

/// The expressions a statement of a simple generator holds directly.
fn stmt_exprs(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::Assign(assign) => vec![&assign.value],
        Stmt::Expr(expr_stmt) => match yield_value(stmt) {
            Some(value) => vec![value],
            None => vec![&expr_stmt.expr],
        },
        Stmt::If(if_stmt) => if_stmt
            .branches
            .iter()
            .map(|branch| &branch.condition)
            .collect(),
        Stmt::While(while_stmt) => vec![&while_stmt.condition],
        Stmt::For(for_stmt) => match &for_stmt.iterable.kind {
            ExprKind::Call { args, .. } => args.iter().map(|arg| &arg.value).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Whether every name `expr` reads is a parameter or local of the
/// generator. A callee keeps its name and is checked by the caller's
/// bindings instead.
fn names_are_fields(expr: &Expr, fields: &BTreeSet<String>) -> bool {
    match &expr.kind {
        ExprKind::Name(name) => fields.contains(name),
        ExprKind::Call { callee, args } => {
            let callee_ok = match &callee.kind {
                ExprKind::Name(_) => true,
                _ => names_are_fields(callee, fields),
            };
            callee_ok && args.iter().all(|arg| names_are_fields(&arg.value, fields))
        }
        _ => {
            let mut all = true;
            let mut probe = expr.clone();
            probe.for_each_child_mut(&mut |child| all &= names_are_fields(child, fields));
            all
        }
    }
}

/// The callee names of a generator's body, which a fused loop reads in the
/// caller's scope.
fn callee_names(body: &[Stmt]) -> BTreeSet<String> {
    fn walk(expr: &Expr, out: &mut BTreeSet<String>) {
        if let ExprKind::Call { callee, .. } = &expr.kind {
            if let ExprKind::Name(name) = &callee.kind {
                out.insert(name.clone());
            }
        }
        let mut probe = expr.clone();
        probe.for_each_child_mut(&mut |child| walk(child, out));
    }
    let mut out = BTreeSet::new();
    visit_stmts(body, &mut |stmt| {
        for expr in stmt_exprs(stmt) {
            walk(expr, &mut out);
        }
    });
    out
}

/// Names `function` binds anywhere: its parameters and the names it
/// assigns or loops over.
fn bound_names(function: &FunctionDecl) -> BTreeSet<String> {
    fn target_names(target: &BindingTarget, out: &mut BTreeSet<String>) {
        match target {
            BindingTarget::Name { name, .. } | BindingTarget::Rest { name, .. } => {
                out.insert(name.clone());
            }
            BindingTarget::Tuple { elements, .. } => {
                for element in elements {
                    target_names(element, out);
                }
            }
        }
    }
    let mut out = function
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect::<BTreeSet<_>>();
    visit_stmts(&function.body, &mut |stmt| match stmt {
        Stmt::Assign(assign) => {
            if let AssignTarget::Name(name) = &assign.target {
                out.insert(name.clone());
            }
        }
        Stmt::For(for_stmt) => target_names(&for_stmt.target, &mut out),
        Stmt::Destructure(destructure) => target_names(&destructure.target, &mut out),
        _ => {}
    });
    out
}

/// Whether `body` holds a `break` that would leave the loop it is the body
/// of, or a `continue` for that loop.
fn loose_jumps(body: &[Stmt]) -> (bool, bool) {
    let mut breaks = false;
    let mut continues = false;
    for stmt in body {
        match stmt {
            Stmt::Break(_) => breaks = true,
            Stmt::Continue(_) => continues = true,
            Stmt::If(if_stmt) => {
                for inner in if_stmt
                    .branches
                    .iter()
                    .map(|branch| branch.body.as_slice())
                    .chain(if_stmt.else_body.as_deref())
                {
                    let (b, c) = loose_jumps(inner);
                    breaks |= b;
                    continues |= c;
                }
            }
            Stmt::Match(match_stmt) => {
                for arm in &match_stmt.arms {
                    let (b, c) = loose_jumps(&arm.body);
                    breaks |= b;
                    continues |= c;
                }
            }
            Stmt::With(with) => {
                let (b, c) = loose_jumps(&with.body);
                breaks |= b;
                continues |= c;
            }
            _ => {}
        }
    }
    (breaks, continues)
}

/// Replaces loops over fusable generators with the generator's body.
struct Fuser<'f> {
    fusable: &'f BTreeMap<String, Fusable>,
    bound: &'f BTreeSet<String>,
    columns: &'f mut usize,
    count: usize,
}

impl Fuser<'_> {
    fn block(&mut self, body: &mut Vec<Stmt>) {
        let mut index = 0;
        while index < body.len() {
            self.nested(&mut body[index]);
            let Some(replacement) = self.fuse(&body[index]) else {
                index += 1;
                continue;
            };
            body.remove(index);
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
            Stmt::While(while_stmt) => self.block(&mut while_stmt.body),
            Stmt::For(for_stmt) => self.block(&mut for_stmt.body),
            Stmt::Match(match_stmt) => {
                for arm in &mut match_stmt.arms {
                    self.block(&mut arm.body);
                }
            }
            Stmt::With(with) => self.block(&mut with.body),
            _ => {}
        }
    }

    /// The fused form of `stmt` when it loops over a fusable generator:
    ///
    /// ```text
    /// mut <fuse> param = argument     # each parameter, in order
    /// <the generator's body, its names renamed, with its `yield value` as>
    ///     target = value
    ///     <the loop body>              # in a one-pass loop if it continues
    /// ```
    fn fuse(&mut self, stmt: &Stmt) -> Option<Vec<Stmt>> {
        let Stmt::For(for_stmt) = stmt else {
            return None;
        };
        let BindingTarget::Name { name: target, .. } = &for_stmt.target else {
            return None;
        };
        let ExprKind::Call { callee, args } = &for_stmt.iterable.kind else {
            return None;
        };
        let ExprKind::Name(generator) = &callee.kind else {
            return None;
        };
        let fusable = self.fusable.get(generator)?;
        if target == "_"
            || for_stmt.borrow_mode.is_some()
            || !for_stmt.contracts.is_empty()
            || self.bound.contains(generator)
            || args.len() != fusable.params.len()
            || args.iter().any(|arg| arg.name.is_some())
            || callee_names(&fusable.body)
                .iter()
                .any(|name| self.bound.contains(name))
        {
            return None;
        }
        let (breaks, continues) = loose_jumps(&for_stmt.body);
        let span = for_stmt.span;
        let prefix = format!("fuse {}:{} ", span.line, span.column);
        let rename = |name: &str| format!("{prefix}{name}");
        let mut out = Vec::new();
        for (param, arg) in fusable.params.iter().zip(args) {
            out.push(assign(&rename(param), arg.value.clone(), true, span));
        }
        // A `break` in the loop body and a `return` in the generator both
        // end the whole loop. Each sets a flag and leaves its innermost
        // loop; every loop around it checks the flag when it ends, and the
        // generator's body runs in a one-pass loop that the outermost
        // `break` leaves.
        let stop = rename("stop");
        let exits = breaks || fusable.returns;
        if exits {
            out.push(assign(
                &stop,
                Expr {
                    kind: ExprKind::Bool(false),
                    span,
                },
                true,
                span,
            ));
        }
        let mut loop_body = for_stmt.body.clone();
        if breaks {
            stop_on_break(&mut loop_body, &stop, span);
        }
        if continues {
            // `continue` ends this item's pass through the body, as it
            // would in the loop.
            loop_body = vec![Stmt::For(crate::ast::ForStmt {
                target: BindingTarget::Name {
                    name: format!("{prefix}pass"),
                    span,
                },
                iterable: call(name_expr("range", span), vec![int_expr(1, span)], span),
                borrow_mode: None,
                contracts: Vec::new(),
                body: loop_body,
                span,
            })];
            if breaks {
                loop_body.push(break_if(&stop, span));
            }
        }
        let mut generated = fusable.body.clone();
        let renamed = fusable
            .fields
            .iter()
            .map(|field| (field.clone(), rename(field)))
            .collect::<BTreeMap<_, _>>();
        rename_body(&mut generated, &renamed);
        renumber(&mut generated, self.columns);
        if fusable.returns {
            stop_on_return(&mut generated, &stop, span);
        }
        let exit = exits.then_some(stop.as_str());
        let body = place_loop_body(generated, target, &mut Some(loop_body), exit);
        if exits {
            out.push(one_pass(body, &format!("{prefix}once"), span));
        } else {
            out.extend(body);
        }
        Some(out)
    }
}

/// `body` with its one `yield value` replaced by `target = value` and the
/// loop body.
fn place_loop_body(
    body: Vec<Stmt>,
    target: &str,
    loop_body: &mut Option<Vec<Stmt>>,
    exit: Option<&str>,
) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(body.len());
    for mut stmt in body {
        // A loop that holds an exit checks the flag when it ends.
        let check = exit.filter(|_| {
            matches!(stmt, Stmt::While(_) | Stmt::For(_))
                && (loop_body.is_some() && holds_yield(std::slice::from_ref(&stmt))
                    || holds_stop(std::slice::from_ref(&stmt)))
        });
        if let Some(value) = yield_value(&stmt) {
            let span = value.span;
            out.push(assign(target, value.clone(), false, span));
            out.extend(loop_body.take().expect("a fusable generator yields once"));
            continue;
        }
        match &mut stmt {
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    branch.body =
                        place_loop_body(std::mem::take(&mut branch.body), target, loop_body, exit);
                }
                if let Some(else_body) = &mut if_stmt.else_body {
                    *else_body =
                        place_loop_body(std::mem::take(else_body), target, loop_body, exit);
                }
            }
            Stmt::While(while_stmt) => {
                while_stmt.body = place_loop_body(
                    std::mem::take(&mut while_stmt.body),
                    target,
                    loop_body,
                    exit,
                );
            }
            Stmt::For(for_stmt) => {
                for_stmt.body =
                    place_loop_body(std::mem::take(&mut for_stmt.body), target, loop_body, exit);
            }
            _ => {}
        }
        let span = stmt_span(&stmt);
        out.push(stmt);
        if let Some(stop) = check {
            out.push(break_if(stop, span));
        }
    }
    out
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::While(while_stmt) => while_stmt.span,
        Stmt::For(for_stmt) => for_stmt.span,
        _ => Span::new(0, 0),
    }
}

/// Whether `body` holds the generator's `yield`.
fn holds_yield(body: &[Stmt]) -> bool {
    let mut found = false;
    visit_stmts(body, &mut |stmt| found |= yield_value(stmt).is_some());
    found
}

/// Whether `body` holds a `return` that `stop_on_return` rewrote.
fn holds_stop(body: &[Stmt]) -> bool {
    let mut found = false;
    visit_stmts(body, &mut |stmt| {
        found |= matches!(stmt, Stmt::Assign(assign)
            if matches!(&assign.target, AssignTarget::Name(name) if name.ends_with(" stop"))
                && matches!(assign.value.kind, ExprKind::Bool(true)));
    });
    found
}

/// `if <stop>: break`.
fn break_if(stop: &str, span: Span) -> Stmt {
    Stmt::If(IfStmt {
        branches: vec![IfBranch {
            condition: name_expr(stop, span),
            body: vec![Stmt::Break(crate::ast::BreakStmt { span })],
            span,
        }],
        else_body: None,
        span,
    })
}

/// `<stop> = true` and `break`.
fn stop_and_break(stop: &str, span: Span) -> [Stmt; 2] {
    [
        assign(
            stop,
            Expr {
                kind: ExprKind::Bool(true),
                span,
            },
            false,
            span,
        ),
        Stmt::Break(crate::ast::BreakStmt { span }),
    ]
}

/// Rewrites each `break` of the loop body itself into `stop_and_break`.
fn stop_on_break(body: &mut Vec<Stmt>, stop: &str, span: Span) {
    let mut index = 0;
    while index < body.len() {
        match &mut body[index] {
            Stmt::Break(_) => {
                body.splice(index..=index, stop_and_break(stop, span));
                index += 2;
                continue;
            }
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    stop_on_break(&mut branch.body, stop, span);
                }
                if let Some(else_body) = &mut if_stmt.else_body {
                    stop_on_break(else_body, stop, span);
                }
            }
            Stmt::Match(match_stmt) => {
                for arm in &mut match_stmt.arms {
                    stop_on_break(&mut arm.body, stop, span);
                }
            }
            Stmt::With(with) => stop_on_break(&mut with.body, stop, span),
            _ => {}
        }
        index += 1;
    }
}

/// Rewrites each `return` of the generator into `stop_and_break`.
fn stop_on_return(body: &mut Vec<Stmt>, stop: &str, span: Span) {
    let mut index = 0;
    while index < body.len() {
        match &mut body[index] {
            Stmt::Return(_) => {
                body.splice(index..=index, stop_and_break(stop, span));
                index += 2;
                continue;
            }
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    stop_on_return(&mut branch.body, stop, span);
                }
                if let Some(else_body) = &mut if_stmt.else_body {
                    stop_on_return(else_body, stop, span);
                }
            }
            Stmt::While(while_stmt) => stop_on_return(&mut while_stmt.body, stop, span),
            Stmt::For(for_stmt) => stop_on_return(&mut for_stmt.body, stop, span),
            _ => {}
        }
        index += 1;
    }
}

/// `for <name> in range(1): body`, a loop that runs `body` once and that
/// `break` and `continue` leave.
fn one_pass(body: Vec<Stmt>, name: &str, span: Span) -> Stmt {
    Stmt::For(crate::ast::ForStmt {
        target: BindingTarget::Name {
            name: name.to_string(),
            span,
        },
        iterable: call(name_expr("range", span), vec![int_expr(1, span)], span),
        borrow_mode: None,
        contracts: Vec::new(),
        body,
        span,
    })
}

/// Renames the generator's parameters and locals in `body`.
fn rename_body(body: &mut [Stmt], renamed: &BTreeMap<String, String>) {
    fn expr(value: &mut Expr, renamed: &BTreeMap<String, String>) {
        match &mut value.kind {
            ExprKind::Name(name) => {
                if let Some(new) = renamed.get(name) {
                    *name = new.clone();
                }
            }
            ExprKind::Call { callee, args } => {
                if !matches!(callee.kind, ExprKind::Name(_)) {
                    expr(callee, renamed);
                }
                for arg in args {
                    expr(&mut arg.value, renamed);
                }
            }
            _ => value.for_each_child_mut(&mut |child| expr(child, renamed)),
        }
    }
    for stmt in body {
        match stmt {
            Stmt::Assign(assign) => {
                if let AssignTarget::Name(name) = &mut assign.target {
                    if let Some(new) = renamed.get(name) {
                        *name = new.clone();
                    }
                }
                expr(&mut assign.value, renamed);
            }
            Stmt::Expr(expr_stmt) => expr(&mut expr_stmt.expr, renamed),
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    expr(&mut branch.condition, renamed);
                    rename_body(&mut branch.body, renamed);
                }
                if let Some(body) = &mut if_stmt.else_body {
                    rename_body(body, renamed);
                }
            }
            Stmt::While(while_stmt) => {
                expr(&mut while_stmt.condition, renamed);
                rename_body(&mut while_stmt.body, renamed);
            }
            Stmt::For(for_stmt) => {
                if let BindingTarget::Name { name, .. } = &mut for_stmt.target {
                    if let Some(new) = renamed.get(name) {
                        *name = new.clone();
                    }
                }
                expr(&mut for_stmt.iterable, renamed);
                rename_body(&mut for_stmt.body, renamed);
            }
            _ => {}
        }
    }
}
