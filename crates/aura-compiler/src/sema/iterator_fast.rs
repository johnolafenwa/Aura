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

/// The module with the fast forms added and its context, or `None` when
/// nothing in the module qualifies.
///
/// `extended` also copies functions that generators are passed to (see
/// `iterator_fast_args.rs`) and gives frames to generators with collection
/// and class locals and loops over lists and other generators. A module
/// whose extended forms do not check is tried with the others alone.
pub(super) fn expand(
    module: &Module,
    context: &ModuleContext,
    extended: bool,
) -> Option<(Module, ModuleContext)> {
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
    let specialized = if extended {
        super::iterator_fast_args::specialize(&mut module, &mut columns)
    } else {
        0
    };
    let mut advances = Vec::new();
    for item in &mut module.items {
        let Item::Impl(impl_decl) = item else {
            continue;
        };
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
    let mut framed = BTreeMap::new();
    let mut scope = Scope::of(&module, extended);
    // A generator that loops over another generator gets a frame once that
    // generator has one, so the module is walked until nothing changes.
    loop {
        let mut changed = false;
        for item in &module.items {
            let Item::Function(function) = item else {
                continue;
            };
            if framed.contains_key(&function.name) {
                continue;
            }
            let Some(frame) = generator_frame(function, &scope, &mut columns) else {
                continue;
            };
            changed = true;
            windows += 1;
            frames.extend(frame);
            framed.insert(function.name.clone(), function.params.len());
            let item = generator_item_key(function).expect("framed generators have an item type");
            let params = function
                .params
                .iter()
                .map(|param| type_key(&param.ty).expect("framed parameters have keys"))
                .collect();
            scope
                .generators
                .insert(function.name.clone(), (item, params));
        }
        for item in &module.items {
            let Item::Function(function) = item else {
                continue;
            };
            if !framed.contains_key(&function.name) || fusable.contains_key(&function.name) {
                continue;
            }
            if let Some(found) = Fusable::of(function, &scope) {
                changed = true;
                scope.fusable.insert(function.name.clone());
                fusable.insert(function.name.clone(), found);
            }
        }
        if !changed {
            break;
        }
    }
    let mut fused = 0;
    if !framed.is_empty() {
        for item in &mut module.items {
            let bodies: Vec<&mut FunctionDecl> = match item {
                Item::Function(function) => vec![function],
                Item::Class(class) => class.methods.iter_mut().collect(),
                Item::Impl(impl_decl) => impl_decl.methods.iter_mut().collect(),
                _ => Vec::new(),
            };
            for function in bodies {
                fused += hoist_held_generators(function, &framed);
            }
        }
    }
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
    if windows == 0 && fused == 0 && specialized == 0 {
        return None;
    }
    // Without any extended form, the other forms alone are the same module.
    if extended && specialized == 0 && !scope.used.get() {
        return None;
    }
    module.items.extend(frames);
    let mut context = context.clone();
    let namespace = crate::builtin_sources::importable_builtin_namespace(&[
        super::iterator_loops::ITER_MODULE.to_string(),
    ])
    .expect("the builtin `iter` module is importable");
    context
        .imported_bindings
        .insert(ITER_ALIAS.to_string(), ImportedBinding::Module(namespace));
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

/// What the frame pass knows about the module around a generator: its
/// classes, the generators that already have frames, and the declared
/// result of each module function. Frame fields name their types by a key,
/// the written type's text (`int64`, `list[str]`, `Point`).
#[derive(Default)]
pub(super) struct Scope {
    /// Each non-generic class: its fields' names, type keys, and whether
    /// they have a default.
    classes: BTreeMap<String, Vec<(String, Option<String>, bool)>>,
    /// Each generator with a frame: its item's key and its parameters'
    /// keys.
    generators: BTreeMap<String, (String, Vec<String>)>,
    /// Each fusable generator.
    fusable: BTreeSet<String>,
    /// The result key of each module function that is not a generator.
    results: BTreeMap<String, String>,
    /// Whether frames may hold collections, classes, and other frames.
    extended: bool,
    /// Whether an accepted form needed `extended`.
    used: std::cell::Cell<bool>,
}

/// Builtin collections a frame field may hold.
const COLLECTIONS: &[(&str, usize)] = &[("list", 1), ("dict", 2), ("set", 1)];

impl Scope {
    pub(super) fn of(module: &Module, extended: bool) -> Self {
        let mut scope = Self {
            extended,
            ..Self::default()
        };
        if !extended {
            return scope;
        }
        for item in &module.items {
            match item {
                Item::Class(class) if class.type_params.is_empty() => {
                    let fields = class
                        .fields
                        .iter()
                        .map(|field| {
                            (
                                field.name.clone(),
                                type_key(&field.ty),
                                field.default.is_some(),
                            )
                        })
                        .collect();
                    scope.classes.insert(class.name.clone(), fields);
                }
                Item::Function(function)
                    if function.type_params.is_empty()
                        && !super::generators::body_yields(&function.body) =>
                {
                    if let Some(key) = type_key(&function.return_type) {
                        scope.results.insert(function.name.clone(), key);
                    }
                }
                _ => {}
            }
        }
        scope
    }

    /// Whether a field may hold a value of type `key`: a number, `bool`,
    /// `str`, a builtin collection of those, a class of this module, or
    /// another generator's frame.
    pub(super) fn holds(&self, key: &str) -> bool {
        if SCALARS.contains(&key) || key == "str" {
            return true;
        }
        self.extended && self.mark(self.zero(key, Span::new(0, 0), 0).is_some())
    }

    /// A value of type `key` that runs nothing: the starting value of a
    /// frame field before the generator's body assigns it.
    fn zero(&self, key: &str, span: Span, depth: usize) -> Option<Expr> {
        if depth > 8 {
            return None;
        }
        let (name, args) = parse_key(key)?;
        if args.is_empty() {
            if SCALARS.contains(&name.as_str()) || name == "str" {
                return Some(zero(&name, span));
            }
            if let Some(generator) = name.strip_prefix(FRAME_CLASS_PREFIX) {
                let (_, params) = self.generators.get(generator)?;
                let args = params
                    .iter()
                    .map(|param| self.zero(param, span, depth + 1))
                    .collect::<Option<Vec<_>>>()?;
                return Some(call(
                    name_expr(&format!("{FRAME_NEW_PREFIX}{generator}"), span),
                    args,
                    span,
                ));
            }
            let fields = self.classes.get(&name)?;
            let mut args = Vec::new();
            for (field, ty, default) in fields {
                if *default {
                    continue;
                }
                let value = self.zero(ty.as_deref()?, span, depth + 1)?;
                args.push(Argument {
                    name: Some(field.clone()),
                    value,
                    span,
                });
            }
            return Some(Expr {
                kind: ExprKind::Call {
                    callee: Box::new(name_expr(&name, span)),
                    args,
                },
                span,
            });
        }
        let arity = COLLECTIONS
            .iter()
            .find(|(collection, _)| *collection == name)
            .map(|(_, arity)| *arity)?;
        if args.len() != arity || !args.iter().all(|arg| self.holds_element(arg, depth)) {
            return None;
        }
        let kind = match name.as_str() {
            "list" => ExprKind::List(Vec::new()),
            "set" => ExprKind::Set(Vec::new()),
            _ => ExprKind::Map(Vec::new()),
        };
        Some(Expr { kind, span })
    }

    /// Records that a form only extended frames take was accepted.
    fn mark(&self, accepted: bool) -> bool {
        if accepted {
            self.used.set(true);
        }
        accepted
    }

    fn holds_element(&self, key: &str, depth: usize) -> bool {
        self.zero(key, Span::new(0, 0), depth + 1).is_some()
    }

    /// The key of the items of a `for` loop over a value of type `key`, when
    /// a frame can step through it by index: a list of numbers, `bool`, or
    /// `str`.
    pub(super) fn indexed_item(key: &str) -> Option<String> {
        let (name, args) = parse_key(key)?;
        (name == "list" && args.len() == 1)
            .then(|| args[0].clone())
            .filter(|item| SCALARS.contains(&item.as_str()) || item == "str")
    }

    /// The key of a list's elements.
    pub(super) fn element(key: &str) -> Option<String> {
        let (name, args) = parse_key(key)?;
        (name == "list" && args.len() == 1).then(|| args[0].clone())
    }
}

/// The key of the item type of a generator function.
fn generator_item_key(function: &FunctionDecl) -> Option<String> {
    let TypeRefKind::Named { args, .. } = &function.return_type.kind else {
        return None;
    };
    type_key(args.first()?)
}

/// The key of a written type: a name with optional type arguments, as
/// written. Unions, tuples, and callables have none.
pub(super) fn type_key(ty: &TypeRef) -> Option<String> {
    let TypeRefKind::Named { name, args } = &ty.kind else {
        return None;
    };
    if ty.indirect {
        return None;
    }
    if args.is_empty() {
        return Some(name.clone());
    }
    let args = args.iter().map(type_key).collect::<Option<Vec<_>>>()?;
    Some(format!("{name}[{}]", args.join(", ")))
}

/// A key's name and type argument keys.
pub(super) fn parse_key(key: &str) -> Option<(String, Vec<String>)> {
    let Some(open) = key.find('[') else {
        return Some((key.to_string(), Vec::new()));
    };
    let inner = key.strip_suffix(']')?.get(open + 1..)?;
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (at, ch) in inner.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                args.push(inner[start..at].trim().to_string());
                start = at + 1;
            }
            _ => {}
        }
    }
    args.push(inner[start..].trim().to_string());
    Some((key[..open].to_string(), args))
}

/// The written type a key names.
pub(super) fn key_type(key: &str, span: Span) -> TypeRef {
    let (name, args) = parse_key(key).unwrap_or_else(|| (key.to_string(), Vec::new()));
    TypeRef::named(
        name,
        args.iter().map(|arg| key_type(arg, span)).collect(),
        false,
        span,
    )
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
    let next = impl_decl.methods.iter_mut().find(|method| {
        method.name == super::iterator_loops::ITERATOR_NEXT
            && method.receiver == Some(ReceiverKind::BorrowMut)
            && method.params.is_empty()
    })?;
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
            let rewritten = ret.value.as_ref().and_then(|value| {
                Some(match step_names.step_value(value)? {
                    None => name_expr("None", value.span),
                    Some(item) => item.clone(),
                })
            });
            rewritten.map(|value| ret.value = Some(value)).is_some()
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
    fn infer(&self, expr: &Expr, scope: &Scope) -> Option<String> {
        match &expr.kind {
            ExprKind::Int(_) => Some("int64".to_string()),
            ExprKind::Float(_) => Some("float64".to_string()),
            ExprKind::Bool(_) => Some("bool".to_string()),
            ExprKind::String(_) | ExprKind::FString(_) => Some("str".to_string()),
            ExprKind::Name(name) => self.ty(name).map(str::to_string),
            ExprKind::Group(inner) => self.infer(inner, scope),
            ExprKind::Cast { ty, .. } => scalar_name(ty).map(str::to_string),
            ExprKind::Unary { op, expr } => match op {
                UnaryOp::Not => Some("bool".to_string()),
                UnaryOp::Neg | UnaryOp::BitNot => self.infer(expr, scope),
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
                    let found = match (literal(left), literal(right)) {
                        (true, false) => self.infer(right, scope),
                        (false, true) => self.infer(left, scope),
                        _ => {
                            let left = self.infer(left, scope)?;
                            (self.infer(right, scope)? == left).then_some(left)
                        }
                    }?;
                    // Only numbers and `str` take arithmetic here.
                    (SCALARS.contains(&found.as_str()) || found == "str").then_some(found)
                }
            },
            ExprKind::CompareChain { .. } | ExprKind::IsNone { .. } => Some("bool".to_string()),
            ExprKind::Call { callee, args } => match &callee.kind {
                // A class of this module builds an instance of itself.
                ExprKind::Name(name)
                    if scope.classes.contains_key(name) && self.ty(name).is_none() =>
                {
                    Some(name.clone())
                }
                ExprKind::Name(name) if self.ty(name).is_none() => scope.results.get(name).cloned(),
                ExprKind::Member { object, field }
                    if scope.extended && field == "len" && args.is_empty() =>
                {
                    let object = self.infer(object, scope)?;
                    let (name, _) = parse_key(&object)?;
                    scope
                        .mark(matches!(name.as_str(), "list" | "dict" | "set" | "str"))
                        .then(|| "int64".to_string())
                }
                _ => None,
            },
            ExprKind::Member { object, field } => {
                let class = self.infer(object, scope)?;
                let fields = scope.classes.get(&class)?;
                fields
                    .iter()
                    .find(|(name, ..)| name == field)
                    .and_then(|(_, ty, _)| ty.clone())
            }
            ExprKind::Index { object, .. } => {
                let object = self.infer(object, scope)?;
                if object == "str" {
                    return Some(object);
                }
                Scope::element(&object)
            }
            ExprKind::List(elements) => {
                let first = self.infer(elements.first()?, scope)?;
                elements
                    .iter()
                    .all(|element| self.infer(element, scope).as_deref() == Some(first.as_str()))
                    .then(|| format!("list[{first}]"))
            }
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

/// The root name of an assignment target that writes through a local:
/// `name.field`, `name[index]`, and deeper paths from `name`.
fn place_root(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Name(name) => Some(name),
        ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => place_root(object),
        _ => None,
    }
}

/// The generator a `for` loop source calls directly, with its arguments,
/// when that generator has a frame.
fn framed_call<'e>(iterable: &'e Expr, scope: &Scope) -> Option<(&'e str, &'e [Argument])> {
    let ExprKind::Call { callee, args } = &iterable.kind else {
        return None;
    };
    let ExprKind::Name(name) = &callee.kind else {
        return None;
    };
    (scope.generators.contains_key(name) && args.iter().all(|arg| arg.name.is_none()))
        .then_some((name.as_str(), args.as_slice()))
}

/// Collects the locals of a simple generator body and their types. False
/// when the body uses anything a frame cannot hold.
fn collect_locals(
    body: &[Stmt],
    layout: &mut FrameLayout,
    loop_targets: &mut BTreeSet<String>,
    scope: &Scope,
) -> bool {
    body.iter().all(|stmt| {
        if let Some(value) = yield_value(stmt) {
            return frame_expr(value);
        }
        match stmt {
            Stmt::Assign(assign) => {
                if !frame_expr(&assign.value) {
                    return false;
                }
                let target = match &assign.target {
                    AssignTarget::Name(target) => target,
                    _ if !scope.extended => return false,
                    // A write through a local: its field or element.
                    AssignTarget::Member { object, .. } => {
                        return scope.mark(true)
                            && assign.annotation.is_none()
                            && frame_expr(object)
                            && place_root(object).is_some_and(|root| {
                                layout.ty(root).is_some() && !loop_targets.contains(root)
                            });
                    }
                    AssignTarget::Index { object, index } => {
                        return scope.mark(true)
                            && assign.annotation.is_none()
                            && frame_expr(object)
                            && frame_expr(index)
                            && place_root(object).is_some_and(|root| {
                                layout.ty(root).is_some() && !loop_targets.contains(root)
                            });
                    }
                };
                if loop_targets.contains(target) {
                    return false;
                }
                if assign.op.is_some() {
                    return layout.ty(target).is_some();
                }
                let ty = match &assign.annotation {
                    Some(annotation) => type_key(annotation),
                    None => layout.infer(&assign.value, scope),
                };
                ty.is_some_and(|ty| scope.holds(&ty) && layout.declare(target, ty))
            }
            Stmt::Expr(expr_stmt) => frame_expr(&expr_stmt.expr),
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Return(ret) => ret.value.is_none(),
            Stmt::If(if_stmt) => {
                if_stmt.branches.iter().all(|branch| {
                    frame_expr(&branch.condition)
                        && collect_locals(&branch.body, layout, loop_targets, scope)
                }) && if_stmt
                    .else_body
                    .as_ref()
                    .is_none_or(|body| collect_locals(body, layout, loop_targets, scope))
            }
            Stmt::While(while_stmt) => {
                while_stmt.contracts.is_empty()
                    && frame_expr(&while_stmt.condition)
                    && collect_locals(&while_stmt.body, layout, loop_targets, scope)
            }
            Stmt::For(for_stmt) => {
                let BindingTarget::Name { name, .. } = &for_stmt.target else {
                    return false;
                };
                if for_stmt.borrow_mode.is_some()
                    || !for_stmt.contracts.is_empty()
                    || layout.ty(name).is_some()
                {
                    return false;
                }
                let item = if let Some((start, end)) = range_bounds(&for_stmt.iterable) {
                    let int64 = |expr: &Expr| {
                        matches!(expr.kind, ExprKind::Int(_))
                            || layout.infer(expr, scope).as_deref() == Some("int64")
                    };
                    if !frame_expr(&start) || !frame_expr(&end) || !int64(&start) || !int64(&end) {
                        return false;
                    }
                    "int64".to_string()
                } else if let Some((generator, args)) = framed_call(&for_stmt.iterable, scope)
                    .filter(|_| scope.extended && scope.mark(true))
                {
                    let (item, params) = &scope.generators[generator];
                    if args.len() != params.len() || !args.iter().all(|arg| frame_expr(&arg.value))
                    {
                        return false;
                    }
                    item.clone()
                } else if let ExprKind::Name(source) = &for_stmt.iterable.kind {
                    // A list local or parameter, stepped through by index.
                    let Some(item) = layout.ty(source).and_then(Scope::indexed_item) else {
                        return false;
                    };
                    item
                } else {
                    return false;
                };
                if !layout.declare(name, item) {
                    return false;
                }
                layout.loops += 1;
                loop_targets.insert(name.clone());
                let ok = collect_locals(&for_stmt.body, layout, loop_targets, scope);
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
fn generator_frame(
    function: &FunctionDecl,
    scope: &Scope,
    columns: &mut usize,
) -> Option<Vec<Item>> {
    let TypeRefKind::Named { name, args } = &function.return_type.kind else {
        return None;
    };
    if name.rsplit('.').next() != Some("Generator") || args.len() != 1 {
        return None;
    }
    let item = args[0].clone();
    let classes = scope.classes.keys().cloned().collect::<BTreeSet<_>>();
    if !none_free(&item, &classes) {
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
        let ty = type_key(&param.ty).filter(|ty| scope.holds(ty))?;
        (param.mode != ParamMode::BorrowMut && layout.declare(&param.name, ty)).then_some(())?;
    }
    if !collect_locals(&function.body, &mut layout, &mut BTreeSet::new(), scope) {
        return None;
    }
    let span = function.span;
    let fields = layout.locals.keys().cloned().collect::<BTreeSet<_>>();

    let mut machine = Machine {
        blocks: vec![Block::default()],
        current: 0,
        loops: Vec::new(),
        fields: &fields,
        layout: &layout,
        scope,
        counters: 0,
        subs: Vec::new(),
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
    let subs = machine.subs.clone();
    for (index, generator) in subs.iter().enumerate() {
        class_fields.push(field(
            &sub_name(index),
            &format!("{FRAME_CLASS_PREFIX}{generator}"),
            span,
        ));
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
            scope.zero(ty, span, 0)?
        };
        inits.push((name.clone(), value));
    }
    inits.push((STATE.to_string(), int_expr(0, span)));
    for counter in 0..machine.counters {
        inits.push((counter_name(counter), int_expr(0, span)));
        inits.push((end_name(counter), int_expr(0, span)));
    }
    for (index, generator) in subs.iter().enumerate() {
        let key = format!("{FRAME_CLASS_PREFIX}{generator}");
        inits.push((sub_name(index), scope.zero(&key, span, 0)?));
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

fn sub_name(index: usize) -> String {
    format!("__aura_sub_{index}")
}

fn sub_step_name(index: usize) -> String {
    format!("sub step {index}")
}

fn zero(ty: &str, span: Span) -> Expr {
    let kind = match ty {
        "bool" => ExprKind::Bool(false),
        "float32" | "float64" => ExprKind::Float(0.0),
        "str" => ExprKind::String(String::new()),
        _ => ExprKind::Int(0),
    };
    Expr { kind, span }
}

fn field(name: &str, ty: &str, span: Span) -> FieldDecl {
    FieldDecl {
        public: false,
        name: name.to_string(),
        ty: key_type(ty, span),
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
    layout: &'a FrameLayout,
    scope: &'a Scope,
    counters: usize,
    /// The generator each loop over a generator call steps, in order.
    subs: Vec<String>,
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

    /// A loop over a call of a generator that has a frame: the frame is a
    /// hidden field, advanced once per item.
    ///
    /// ```text
    /// self.<sub> = __aura_frame_new_<generator>(args)
    /// head:  <step> = self.<sub>.__aura_advance()
    ///        if <step> is not None: self.target = <step>
    ///        branch <step> is not None -> body, exit
    /// ```
    fn lower_sub_loop(&mut self, target: &str, generator: &str, args: &[Argument], body: &[Stmt]) {
        let span = self.span;
        let index = self.subs.len();
        self.subs.push(generator.to_string());
        let sub = sub_name(index);
        let step_local = sub_step_name(index);
        let args = args.iter().map(|arg| self.framed(&arg.value)).collect();
        self.push(set_field(
            &sub,
            call(
                name_expr(&format!("{FRAME_NEW_PREFIX}{generator}"), span),
                args,
                span,
            ),
            span,
        ));
        let head = self.block();
        let body_block = self.block();
        let exit = self.block();
        self.jump_to(Terminator::Goto(head), head);
        self.push(assign(
            &step_local,
            method_call(self_field(&sub, span), ADVANCE, span),
            false,
            span,
        ));
        let not_none = |span| Expr {
            kind: ExprKind::IsNone {
                value: Box::new(name_expr(&step_local, span)),
                negated: true,
                operator_span: span,
            },
            span,
        };
        self.push(Stmt::If(IfStmt {
            branches: vec![IfBranch {
                condition: not_none(span),
                body: vec![set_field(target, name_expr(&step_local, span), span)],
                span,
            }],
            else_body: None,
            span,
        }));
        self.jump_to(
            Terminator::Branch(not_none(span), body_block, exit),
            body_block,
        );
        self.loops.push((head, exit));
        self.lower(body);
        self.loops.pop();
        self.jump_to(Terminator::Goto(head), exit);
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
                let target = match &assign.target {
                    AssignTarget::Name(target) => AssignTarget::Member {
                        object: Box::new(name_expr("self", span)),
                        field: target.clone(),
                    },
                    AssignTarget::Member { object, field } => AssignTarget::Member {
                        object: Box::new(self.framed(object)),
                        field: field.clone(),
                    },
                    AssignTarget::Index { object, index } => AssignTarget::Index {
                        object: Box::new(self.framed(object)),
                        index: Box::new(self.framed(index)),
                    },
                };
                let value = self.framed(&assign.value);
                self.push(Stmt::Assign(AssignStmt {
                    mutable: false,
                    target,
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
                if let Some((generator, args)) = framed_call(&for_stmt.iterable, self.scope) {
                    self.lower_sub_loop(name, generator, args, &for_stmt.body);
                    return;
                }
                // A list is stepped through by index; `range` by its bounds.
                let (start, end, list) = match range_bounds(&for_stmt.iterable) {
                    Some((start, end)) => (start, end, None),
                    None => {
                        let ExprKind::Name(list) = &for_stmt.iterable.kind else {
                            unreachable!("a frame body loops over `range`, a list, or a generator");
                        };
                        debug_assert!(self.layout.ty(list).is_some());
                        let len = call(
                            Expr {
                                kind: ExprKind::Member {
                                    object: Box::new(name_expr(list, span)),
                                    field: "len".to_string(),
                                },
                                span,
                            },
                            Vec::new(),
                            span,
                        );
                        (int_expr(0, span), len, Some(list.clone()))
                    }
                };
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
                let item = match &list {
                    Some(list) => {
                        let element = Expr {
                            kind: ExprKind::Index {
                                object: Box::new(self_field(list, span)),
                                index: Box::new(self_field(&counter, span)),
                            },
                            span,
                        };
                        // A `str` item is an independent copy of the
                        // element, which the loop body only reads.
                        let copied = self.layout.ty(list).and_then(Scope::indexed_item);
                        if copied.as_deref() == Some("str") {
                            method_call(element, "clone", span)
                        } else {
                            element
                        }
                    }
                    None => self_field(&counter, span),
                };
                self.push(set_field(name, item, span));
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
pub(super) fn generated_span(span: Span, next: &mut usize) -> Span {
    *next += 1;
    Span::new(
        span.line,
        span.column % GENERATED_COLUMN_STRIDE + *next * GENERATED_COLUMN_STRIDE,
    )
}

/// Gives every generated statement and expression in `body` a span of its
/// own on its line. The checker keys facts about expressions by span, and
/// generated code reads the same names in many places.
pub(super) fn renumber(body: &mut [Stmt], next: &mut usize) {
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
                match &mut assign.target {
                    AssignTarget::Name(_) => {}
                    AssignTarget::Member { object, .. } => expr(object, next),
                    AssignTarget::Index { object, index } => {
                        expr(object, next);
                        expr(index, next);
                    }
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
    /// The generators its loops call, which the caller must not rebind.
    generators: BTreeSet<String>,
    body: Vec<Stmt>,
}

impl Fusable {
    fn of(function: &FunctionDecl, scope: &Scope) -> Option<Self> {
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
            layout.declare(&param.name, type_key(&param.ty)?);
        }
        // The frame pass already accepted this body.
        collect_locals(&function.body, &mut layout, &mut BTreeSet::new(), scope).then_some(())?;
        // Loops over other generators run in place too, once those are
        // fusable; the caller fuses them in turn.
        let mut generators = BTreeSet::new();
        let mut loops_fuse = true;
        let mut writes_elements = false;
        visit_stmts(&function.body, &mut |stmt| match stmt {
            Stmt::For(for_stmt) => {
                if let Some((generator, _)) = framed_call(&for_stmt.iterable, scope) {
                    loops_fuse &= scope.fusable.contains(generator);
                    generators.insert(generator.to_string());
                }
            }
            Stmt::Assign(assign) => {
                writes_elements |= matches!(assign.target, AssignTarget::Index { .. });
            }
            _ => {}
        });
        if !loops_fuse || writes_elements {
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
        // Building an instance of a class of this module cannot trap.
        if callee_names(&function.body)
            .iter()
            .any(|name| name != "range" && !scope.classes.contains_key(name))
            || !trap_free(&function.body)
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
            generators,
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
            for (offset, stmt) in replacement.into_iter().enumerate() {
                body.insert(index + offset, stmt);
            }
            // The fused body may loop over another fusable generator, so
            // the replacement is walked again; each pass replaces a loop
            // over a generator with the loops of the generators it calls,
            // which ends because no fusable generator reaches itself.
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
                .chain(&fusable.generators)
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
                match &mut assign.target {
                    AssignTarget::Name(name) => {
                        if let Some(new) = renamed.get(name) {
                            *name = new.clone();
                        }
                    }
                    AssignTarget::Member { object, .. } => expr(object, renamed),
                    AssignTarget::Index { object, index } => {
                        expr(object, renamed);
                        expr(index, renamed);
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

// ---------------------------------------------------------------------------
// Held generators

/// Rewrites `g = gen(args)` followed, in the same block, by the only use of
/// `g` as `for x in g:` into `for x in gen(...)`, with each argument kept in a
/// hidden local at the assignment. The generator's body does not start
/// before the loop asks for its first item, so this keeps the meaning, and
/// the loop can then take a fast form. Returns how many it rewrote.
fn hoist_held_generators(function: &mut FunctionDecl, framed: &BTreeMap<String, usize>) -> usize {
    let bound = bound_names(function);
    let Ok(text) = serde_json::to_string(&function.body) else {
        return 0;
    };
    let uses = |name: &str| {
        text.matches(&format!("{{\"Name\":{}}}", serde_json::Value::from(name)))
            .count()
    };
    let mut count = 0;
    hoist_block(&mut function.body, framed, &bound, &uses, &mut count);
    count
}

fn hoist_block(
    body: &mut Vec<Stmt>,
    framed: &BTreeMap<String, usize>,
    bound: &BTreeSet<String>,
    uses: &dyn Fn(&str) -> usize,
    count: &mut usize,
) {
    for stmt in body.iter_mut() {
        match stmt {
            Stmt::If(if_stmt) => {
                for branch in &mut if_stmt.branches {
                    hoist_block(&mut branch.body, framed, bound, uses, count);
                }
                if let Some(else_body) = &mut if_stmt.else_body {
                    hoist_block(else_body, framed, bound, uses, count);
                }
            }
            Stmt::While(while_stmt) => {
                hoist_block(&mut while_stmt.body, framed, bound, uses, count)
            }
            Stmt::For(for_stmt) => hoist_block(&mut for_stmt.body, framed, bound, uses, count),
            Stmt::Match(match_stmt) => {
                for arm in &mut match_stmt.arms {
                    hoist_block(&mut arm.body, framed, bound, uses, count);
                }
            }
            Stmt::With(with) => hoist_block(&mut with.body, framed, bound, uses, count),
            _ => {}
        }
    }
    let mut index = 0;
    while index < body.len() {
        let Some((held, generator, args)) = held_generator(&body[index], framed, bound) else {
            index += 1;
            continue;
        };
        // The holder is assigned here and read once, by a later loop in this
        // block.
        let loop_index = (index + 1..body.len()).find(|&later| {
            matches!(&body[later], Stmt::For(for_stmt)
                if for_stmt.borrow_mode.is_none()
                    && matches!(&for_stmt.iterable.kind, ExprKind::Name(name) if *name == held))
        });
        let Some(loop_index) = loop_index.filter(|_| uses(&held) == 2) else {
            index += 1;
            continue;
        };
        let span = stmt_span_of(&body[index]);
        let mut hidden = Vec::with_capacity(args.len());
        let mut assigns = Vec::with_capacity(args.len());
        for (position, arg) in args.into_iter().enumerate() {
            let name = format!("held {}:{} {position}", span.line, span.column);
            assigns.push(assign(&name, arg, true, span));
            hidden.push(name);
        }
        if let Stmt::For(for_stmt) = &mut body[loop_index] {
            let source_span = for_stmt.iterable.span;
            for_stmt.iterable = call(
                name_expr(&generator, source_span),
                hidden
                    .iter()
                    .map(|name| name_expr(name, source_span))
                    .collect(),
                source_span,
            );
        }
        let added = assigns.len();
        body.splice(index..=index, assigns);
        index += added;
        *count += 1;
    }
}

/// `(holder, generator, arguments)` when `stmt` is `holder = generator(...)`
/// for a framed generator called with one positional argument per
/// parameter.
fn held_generator(
    stmt: &Stmt,
    framed: &BTreeMap<String, usize>,
    bound: &BTreeSet<String>,
) -> Option<(String, String, Vec<Expr>)> {
    let Stmt::Assign(assign) = stmt else {
        return None;
    };
    let (AssignTarget::Name(held), None, None) = (&assign.target, &assign.annotation, &assign.op)
    else {
        return None;
    };
    let ExprKind::Call { callee, args } = &assign.value.kind else {
        return None;
    };
    let ExprKind::Name(generator) = &callee.kind else {
        return None;
    };
    (framed.get(generator) == Some(&args.len())
        && !bound.contains(generator)
        && args.iter().all(|arg| arg.name.is_none()))
    .then(|| {
        (
            held.clone(),
            generator.clone(),
            args.iter().map(|arg| arg.value.clone()).collect(),
        )
    })
}

fn stmt_span_of(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Assign(assign) => assign.span,
        _ => Span::new(0, 0),
    }
}

/// Whether the body's expressions call no methods and index nothing, the
/// other ways besides a call that a body runs code that can trap.
fn trap_free(body: &[Stmt]) -> bool {
    fn walk(expr: &Expr) -> bool {
        let own = match &expr.kind {
            ExprKind::Call { callee, .. } => matches!(callee.kind, ExprKind::Name(_)),
            ExprKind::Index { .. } | ExprKind::Slice { .. } => false,
            _ => true,
        };
        let mut children = true;
        let mut probe = expr.clone();
        probe.for_each_child_mut(&mut |child| children &= walk(child));
        own && children
    }
    let mut free = true;
    visit_stmts(body, &mut |stmt| {
        for expr in stmt_exprs(stmt) {
            free &= walk(expr);
        }
    });
    free
}
