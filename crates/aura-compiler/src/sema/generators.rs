//! Generator functions (Batch 9 step 9c).
//!
//! Before checking, this pass rewrites each module-level function whose body
//! contains a `yield` statement
//!
//! ```text
//! def countdown(n: int64) -> Generator[int64]:
//!     mut i = n
//!     while i > 0:
//!         yield i
//!         i -= 1
//! ```
//!
//! into two ordinary declarations:
//!
//! - the body as a private function `__aura_generator_body_countdown` whose
//!   result is `None` and whose `yield e` statements become
//!   `if not __aura_iter.__aura_generator_yield[int64](e): return`
//! - the function itself, which packs its arguments into a closure and
//!   returns `__aura_iter.__aura_generator[int64](...)` without running any
//!   of the body
//!
//! The runtime runs the body on a separate stack the first time the
//! generator's `next` is called, and each `yield` hands one item back to
//! `next`. A `yield` returns false when the generator is being closed, and
//! the rewritten body returns at once, so its `with` exits run.

use super::{Diagnostic, Item, Module, ModuleContext, Result};
use crate::ast::{
    Argument, Expr, ExprKind, ExprStmt, FunctionDecl, IfBranch, IfStmt, LambdaCapture, ParamMode,
    ReturnStmt, Stmt, TypeAliasDecl, TypeRef, TypeRefKind, UnaryOp, YIELD_MARKER,
};
use crate::diag::Span;
use crate::sema::ImportedBinding;
use aura_core::ops::ReceiverKind;

const CODE: &str = "AU2049";

/// The hidden module alias generated code uses for the builtin `iter` module.
const ITER_ALIAS: &str = "__aura_iter";

/// The private alias of `Callable[own def() -> None]` that packs a
/// generator's arguments.
const BODY_ALIAS: &str = "__aura_generator_body";

/// Prefix of the private function that holds a generator's body.
pub(crate) use crate::diag::GENERATOR_BODY_PREFIX;

/// The parameters of a module's generator functions, so a checker error
/// that a borrowed parameter is consumed can explain the generator rule.
#[derive(Default)]
pub(super) struct GeneratorParams {
    params: Vec<(Span, String, String)>,
}

impl GeneratorParams {
    pub(super) fn map_error(&self, mut diagnostic: Diagnostic) -> Diagnostic {
        let Some(span) = diagnostic.span else {
            return diagnostic;
        };
        if diagnostic.message.contains("`__aura_generator_yield`") {
            if let Some((_, types)) = diagnostic.message.split_once("inferred types for `T`: ") {
                let types = types.replace('`', "");
                if let Some((item, found)) = types.split_once(" and ") {
                    diagnostic.message = format!(
                        "`yield` value type mismatch: the generator yields `{item}`, found `{found}`"
                    );
                    return diagnostic;
                }
            }
            diagnostic.message = diagnostic
                .message
                .replace("function `__aura_generator_yield`", "`yield`");
            return diagnostic;
        }
        if diagnostic.code != "AU3002" || !diagnostic.message.contains("is borrowed") {
            return diagnostic;
        }
        let Some((_, param, function)) = self.params.iter().find(|(at, ..)| *at == span) else {
            return diagnostic;
        };
        diagnostic.message = format!(
            "generator parameter `{param}` must be `own`: the body of `{function}` runs after the call returns, so it cannot borrow the caller's value"
        );
        diagnostic.help = vec![format!(
            "declare `{param}: own T`, and pass a clone if the caller still needs the value"
        )];
        diagnostic.edits.clear();
        diagnostic
    }
}

/// Rewrites every generator function in `module`.
pub(super) fn expand(module: &mut Module, context: &mut ModuleContext) -> Result<GeneratorParams> {
    reject_misplaced_yields(module)?;
    reject_bare_yields(module)?;
    let generators = module
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::Function(function) if body_yields(&function.body) => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();
    if generators.is_empty() {
        return Ok(GeneratorParams::default());
    }
    let Item::Function(first) = &module.items[generators[0]] else {
        unreachable!("generator indices name functions");
    };
    let first_span = first.span;
    if context.module_name == super::iterator_loops::ITER_MODULE {
        return Err(Diagnostic::coded_at(
            CODE,
            first_span,
            "the `iter` module cannot declare generator functions",
        ));
    }
    let namespace = crate::builtin_sources::importable_builtin_namespace(&[
        super::iterator_loops::ITER_MODULE.to_string(),
    ])
    .expect("the builtin `iter` module is importable");
    context
        .imported_bindings
        .insert(ITER_ALIAS.to_string(), ImportedBinding::Module(namespace));

    let mut bodies = Vec::new();
    let mut params = GeneratorParams::default();
    for index in generators {
        let Item::Function(function) = &mut module.items[index] else {
            unreachable!("generator indices name functions");
        };
        params.params.extend(
            function
                .params
                .iter()
                .map(|param| (param.span, param.name.clone(), function.name.clone())),
        );
        bodies.push(Item::Function(rewrite(function)?));
    }
    let none = TypeRef {
        kind: TypeRefKind::Named {
            name: "None".to_string(),
            args: Vec::new(),
        },
        indirect: false,
        span: first_span,
    };
    let signature = TypeRef::function_with_view_return(Vec::new(), none, None, first_span);
    module.items.push(Item::TypeAlias(TypeAliasDecl {
        public: false,
        name: BODY_ALIAS.to_string(),
        type_params: Vec::new(),
        type_param_bounds: Default::default(),
        target: TypeRef::callable(false, ReceiverKind::Value, signature, first_span),
        span: first_span,
    }));
    module.items.extend(bodies);
    Ok(params)
}

/// Splits `function` into its packing wrapper (left in place) and its body
/// function (returned).
fn rewrite(function: &mut FunctionDecl) -> Result<FunctionDecl> {
    let item = generator_item_type(function)?;
    if let Some(clause) = function
        .contracts
        .iter()
        .find(|clause| clause.kind == crate::ast::ContractKind::Ensures)
    {
        return Err(Diagnostic::coded_at(
            CODE,
            clause.span,
            "a generator function cannot have `ensures` clauses",
        )
        .with_help("a generator returns before its body runs; state the promise with `assert`"));
    }
    if let Some(param) = function
        .params
        .iter()
        .find(|param| param.mode == ParamMode::BorrowMut)
    {
        return Err(Diagnostic::coded_at(
            CODE,
            param.span,
            format!(
                "generator parameter `{}` cannot be `mut`: the body runs after `{}` returns",
                param.name, function.name
            ),
        )
        .with_help("take the value with `own` and return it from the generator's items"));
    }
    let span = function.span;
    let body_name = format!("{GENERATOR_BODY_PREFIX}{}", function.name);
    let mut body = std::mem::take(&mut function.body);
    rewrite_yields(&mut body, &item)?;

    let mut body_params = function.params.clone();
    for param in &mut body_params {
        param.default = None;
        param.keyword_only = false;
    }
    let body_function = FunctionDecl {
        public: false,
        name: body_name.clone(),
        type_params: function.type_params.clone(),
        type_param_bounds: function.type_param_bounds.clone(),
        receiver: None,
        params: body_params,
        return_type: TypeRef {
            kind: TypeRefKind::Named {
                name: "None".to_string(),
                args: Vec::new(),
            },
            indirect: false,
            span,
        },
        view_return: None,
        body,
        span,
        property: None,
        decorators: Vec::new(),
        doc: None,
        contracts: Vec::new(),
        spec: false,
    };

    // `return __aura_iter.__aura_generator[T](__aura_generator_body(
    //     lambda [own p, ...]: __aura_generator_body_f(p, ...)))`
    let call_body = expr(
        ExprKind::Call {
            callee: Box::new(name(&body_name, span)),
            args: function
                .params
                .iter()
                .map(|param| Argument {
                    name: None,
                    value: name(&param.name, param.span),
                    span: param.span,
                })
                .collect(),
        },
        span,
    );
    let lambda = expr(
        ExprKind::Lambda {
            captures: Some(
                function
                    .params
                    .iter()
                    .map(|param| LambdaCapture {
                        name: param.name.clone(),
                        mode: ParamMode::Own,
                        span: param.span,
                    })
                    .collect(),
            ),
            params: Vec::new(),
            body: Box::new(call_body),
        },
        span,
    );
    let packed = call(name(BODY_ALIAS, span), vec![lambda], span);
    let start = call(
        expr(
            ExprKind::Specialize {
                expr: Box::new(iter_member("__aura_generator", span)),
                type_args: vec![item],
            },
            span,
        ),
        vec![packed],
        span,
    );
    function.body = vec![Stmt::Return(ReturnStmt {
        value: Some(start),
        view: None,
        span,
    })];
    Ok(body_function)
}

/// The `T` of a generator function's declared `Generator[T]` result.
fn generator_item_type(function: &FunctionDecl) -> Result<TypeRef> {
    if let TypeRefKind::Named { name, args } = &function.return_type.kind {
        let short = name.rsplit('.').next().unwrap_or(name);
        if short == "Generator" && args.len() == 1 {
            return Ok(args[0].clone());
        }
    }
    Err(Diagnostic::coded_at(
        CODE,
        function.return_type.span,
        format!(
            "function `{}` uses `yield`, so its result type must be `Generator[T]`",
            function.name
        ),
    )
    .with_help("import it with `from iter import Generator` and declare `-> Generator[T]`"))
}

fn expr(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

fn name(text: &str, span: Span) -> Expr {
    expr(ExprKind::Name(text.to_string()), span)
}

fn iter_member(field: &str, span: Span) -> Expr {
    expr(
        ExprKind::Member {
            object: Box::new(name(ITER_ALIAS, span)),
            field: field.to_string(),
        },
        span,
    )
}

fn call(callee: Expr, args: Vec<Expr>, span: Span) -> Expr {
    expr(
        ExprKind::Call {
            callee: Box::new(callee),
            args: args
                .into_iter()
                .map(|value| Argument {
                    name: None,
                    span: value.span,
                    value,
                })
                .collect(),
        },
        span,
    )
}

/// The value of a parsed `yield value` statement.
fn yield_value(stmt: &Stmt) -> Option<&Expr> {
    let Stmt::Expr(ExprStmt { expr, .. }) = stmt else {
        return None;
    };
    let ExprKind::Call { callee, args } = &expr.kind else {
        return None;
    };
    match &callee.kind {
        ExprKind::Name(name) if name == YIELD_MARKER => args.first().map(|arg| &arg.value),
        _ => None,
    }
}

fn yield_span(stmt: &Stmt) -> Option<Span> {
    match stmt {
        Stmt::Expr(ExprStmt { expr, .. }) => yield_value(stmt).map(|_| expr.span),
        _ => None,
    }
}

/// Calls `visit` on each nested statement block of `stmt`.
fn for_each_block_mut(
    stmt: &mut Stmt,
    visit: &mut dyn FnMut(&mut Vec<Stmt>) -> Result<()>,
) -> Result<()> {
    match stmt {
        Stmt::If(stmt) => {
            for branch in &mut stmt.branches {
                visit(&mut branch.body)?;
            }
            if let Some(body) = &mut stmt.else_body {
                visit(body)?;
            }
        }
        Stmt::Match(stmt) => {
            for arm in &mut stmt.arms {
                visit(&mut arm.body)?;
            }
        }
        Stmt::For(stmt) => visit(&mut stmt.body)?,
        Stmt::With(stmt) => visit(&mut stmt.body)?,
        Stmt::While(stmt) => visit(&mut stmt.body)?,
        _ => {}
    }
    Ok(())
}

fn for_each_block(stmt: &Stmt, visit: &mut dyn FnMut(&[Stmt])) {
    match stmt {
        Stmt::If(stmt) => {
            for branch in &stmt.branches {
                visit(&branch.body);
            }
            if let Some(body) = &stmt.else_body {
                visit(body);
            }
        }
        Stmt::Match(stmt) => {
            for arm in &stmt.arms {
                visit(&arm.body);
            }
        }
        Stmt::For(stmt) => visit(&stmt.body),
        Stmt::With(stmt) => visit(&stmt.body),
        Stmt::While(stmt) => visit(&stmt.body),
        _ => {}
    }
}

/// The span of the first `yield` in `body`, at any depth.
fn first_yield(body: &[Stmt]) -> Option<Span> {
    for stmt in body {
        if let Some(span) = yield_span(stmt) {
            return Some(span);
        }
        let mut found = None;
        for_each_block(stmt, &mut |block| {
            if found.is_none() {
                found = first_yield(block);
            }
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

fn body_yields(body: &[Stmt]) -> bool {
    first_yield(body).is_some()
}

/// Rejects `yield` where no generator function can hold it: in methods and
/// in top-level statements.
fn reject_misplaced_yields(module: &Module) -> Result<()> {
    let misplaced = |span: Span, place: &str| {
        Err(Diagnostic::coded_at(
            CODE,
            span,
            format!("`yield` can only appear in a module-level generator function, not {place}"),
        ))
    };
    if let Some(span) = first_yield(&module.top_level_stmts) {
        return misplaced(span, "in top-level statements");
    }
    for item in &module.items {
        let methods = match item {
            Item::Class(class) => &class.methods,
            Item::Trait(decl) => &decl.methods,
            Item::Impl(decl) => &decl.methods,
            _ => continue,
        };
        for method in methods {
            if let Some(span) = first_yield(&method.body) {
                return misplaced(span, "in a method");
            }
        }
    }
    Ok(())
}

/// Rejects a `yield` with no value in a function declared to return
/// `Generator[T]`. Elsewhere `yield` alone is an ordinary name.
fn reject_bare_yields(module: &Module) -> Result<()> {
    fn bare_yield(body: &[Stmt]) -> Option<Span> {
        for stmt in body {
            if let Stmt::Expr(ExprStmt { expr, .. }) = stmt {
                if matches!(&expr.kind, ExprKind::Name(name) if name == crate::ast::YIELD_KEYWORD) {
                    return Some(expr.span);
                }
            }
            let mut found = None;
            for_each_block(stmt, &mut |block| {
                if found.is_none() {
                    found = bare_yield(block);
                }
            });
            if found.is_some() {
                return found;
            }
        }
        None
    }
    for item in &module.items {
        let Item::Function(function) = item else {
            continue;
        };
        if generator_item_type(function).is_err() {
            continue;
        }
        if let Some(span) = bare_yield(&function.body) {
            return Err(Diagnostic::coded_at(CODE, span, "`yield` needs a value")
                .with_help("write `yield value`, or a bare `return` to end the generator"));
        }
    }
    Ok(())
}

/// Rewrites each `yield value` in `body` and rejects `return value`.
fn rewrite_yields(body: &mut [Stmt], item: &TypeRef) -> Result<()> {
    for stmt in body.iter_mut() {
        if let Stmt::Return(ReturnStmt {
            value: Some(value), ..
        }) = stmt
        {
            return Err(Diagnostic::coded_at(
                CODE,
                value.span,
                "a generator cannot return a value",
            )
            .with_help("`yield` the value, then use a bare `return` to end the generator"));
        }
        if let Some(value) = yield_value(stmt) {
            let value = value.clone();
            let span = match stmt {
                Stmt::Expr(ExprStmt { expr, .. }) => expr.span,
                _ => value.span,
            };
            let yielded = call(
                expr(
                    ExprKind::Specialize {
                        expr: Box::new(iter_member("__aura_generator_yield", span)),
                        type_args: vec![item.clone()],
                    },
                    span,
                ),
                vec![value],
                span,
            );
            *stmt = Stmt::If(IfStmt {
                branches: vec![IfBranch {
                    condition: expr(
                        ExprKind::Unary {
                            op: UnaryOp::Not,
                            expr: Box::new(yielded),
                        },
                        span,
                    ),
                    body: vec![Stmt::Return(ReturnStmt {
                        value: None,
                        view: None,
                        span,
                    })],
                    span,
                }],
                else_body: None,
                span,
            });
            continue;
        }
        for_each_block_mut(stmt, &mut |block| rewrite_yields(block, item))?;
    }
    Ok(())
}
