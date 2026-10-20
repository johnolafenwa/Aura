//! Function decorators (ADR-0053, Batch 6 checkpoint section E).
//!
//! Before checking, this pass rewrites each decorated module-level function
//!
//! ```text
//! @outer
//! @inner
//! def run(n: int64) -> int64:
//!     ...
//! ```
//!
//! into three ordinary declarations:
//!
//! - the body as a private function `__aura_undecorated_run`
//! - a private alias `__aura_decorated_run = Callable[def(n: int64) -> int64]`
//!   that packs the function
//! - a module constant `run: Callable[...] = outer(inner(__aura_decorated_run(
//!   __aura_undecorated_run)))`
//!
//! A call expression evaluates its callee before its argument, so decorator
//! expressions evaluate top to bottom and apply bottom to top. The constant
//! keeps the function's place among the module constants, which fixes its
//! initialization order. The body's recursive calls name `run`, the constant
//! slot, so they reach the decorated function.

use super::{Diagnostic, Item, Module, Result};
use crate::ast::{
    Argument, ConstantDecl, Expr, ExprKind, FunctionDecl, FunctionTypeParam, ParamMode,
    TypeAliasDecl, TypeRef, TypeRefKind,
};
use crate::diag::Span;
use aura_core::ops::ReceiverKind;

const CODE: &str = "AU2047";

/// Prefix of the private function that holds a decorated function's body.
pub(crate) const UNDECORATED_PREFIX: &str = "__aura_undecorated_";

pub(crate) const DECORATED_PREFIX: &str = "__aura_decorated_";

/// The decorator applications in a module, so a checker error at one can
/// name the decorator and the type it must keep.
#[derive(Default)]
pub(super) struct Applications {
    applications: Vec<Application>,
}

struct Application {
    span: Span,
    function: String,
    signature: String,
}

impl Applications {
    pub(super) fn map_error(&self, mut diagnostic: Diagnostic) -> Diagnostic {
        let Some(span) = diagnostic.span else {
            return diagnostic;
        };
        // Only a type mismatch at the application names the expected type.
        if !matches!(diagnostic.code.as_str(), "AU2002" | "AU2015") {
            return diagnostic;
        }
        if let Some(application) = self
            .applications
            .iter()
            .find(|application| application.span == span)
        {
            diagnostic.message = format!(
                "decorator on `{}` must take and return `{}`: {}",
                application.function, application.signature, diagnostic.message
            );
        }
        diagnostic
    }
}

/// Rewrites every decorated function in `module`.
pub(super) fn expand(module: &mut Module) -> Result<Applications> {
    let mut applications = Applications::default();
    let mut items = Vec::with_capacity(module.items.len());
    for item in std::mem::take(&mut module.items) {
        let Item::Function(function) = item else {
            items.push(item);
            continue;
        };
        if function.decorators.is_empty() {
            items.push(Item::Function(function));
            continue;
        }
        let (alias, function, constant) = desugar(function, &mut applications)?;
        let position = module
            .constants
            .iter()
            .position(|existing| {
                (existing.span.line, existing.span.column)
                    > (constant.span.line, constant.span.column)
            })
            .unwrap_or(module.constants.len());
        module.constants.insert(position, constant);
        items.push(Item::TypeAlias(alias));
        items.push(Item::Function(function));
    }
    module.items = items;
    Ok(applications)
}

fn desugar(
    mut function: FunctionDecl,
    applications: &mut Applications,
) -> Result<(TypeAliasDecl, FunctionDecl, ConstantDecl)> {
    let first = function.decorators[0].span;
    let reject = |message: String| Err(Diagnostic::coded_at(CODE, first, message));
    let name = function.name.clone();
    if !function.type_params.is_empty() {
        return reject(format!("decorated function `{name}` cannot be generic"));
    }
    if name == "main" {
        return reject("`main` cannot be decorated".to_string());
    }
    if let Some(param) = function.params.iter().find(|param| param.default.is_some()) {
        return Err(Diagnostic::coded_at(
            CODE,
            param.span,
            format!(
                "decorated function `{name}` cannot give parameter `{}` a default",
                param.name
            ),
        )
        .with_help("a decorator returns a closure, and a closure has no parameter defaults"));
    }
    if function.view_return.is_some() {
        return reject(format!("decorated function `{name}` cannot return a view"));
    }

    let span = function.span;
    let params = function
        .params
        .iter()
        .map(|param| FunctionTypeParam {
            name: Some(param.name.clone()),
            mode: param.mode,
            ty: param.ty.clone(),
            keyword_only: param.keyword_only,
            has_default: false,
            span: param.span,
        })
        .collect();
    let signature =
        TypeRef::function_with_view_return(params, function.return_type.clone(), None, span);
    let callable = TypeRef::callable(false, ReceiverKind::Borrow, signature, span);
    let alias = TypeAliasDecl {
        public: false,
        name: format!("{DECORATED_PREFIX}{name}"),
        type_params: Vec::new(),
        type_param_bounds: Default::default(),
        target: callable.clone(),
        span,
    };

    let undecorated = format!("{UNDECORATED_PREFIX}{name}");
    let mut value = call(
        Expr {
            kind: ExprKind::Name(alias.name.clone()),
            span,
        },
        Expr {
            kind: ExprKind::Name(undecorated.clone()),
            span,
        },
        span,
    );
    let signature = render(&callable);
    // The undecorated function keeps its decorators for hover and docs.
    for decorator in function.decorators.iter().rev() {
        applications.applications.push(Application {
            span: decorator.span,
            function: name.clone(),
            signature: signature.clone(),
        });
        value = call(decorator.expr.clone(), value, decorator.span);
    }
    let constant = ConstantDecl {
        public: function.public,
        name,
        annotation: Some(callable),
        value,
        span,
    };
    function.public = false;
    function.name = undecorated;
    Ok((alias, function, constant))
}

fn call(callee: Expr, argument: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(callee),
            args: vec![Argument {
                name: None,
                value: argument,
                span,
            }],
        },
        span,
    }
}

/// Writes a type as source text.
fn render(ty: &TypeRef) -> String {
    let list = |types: &[TypeRef], separator: &str| {
        types.iter().map(render).collect::<Vec<_>>().join(separator)
    };
    match &ty.kind {
        TypeRefKind::Named { name, args } if args.is_empty() => name.clone(),
        TypeRefKind::Named { name, args } => format!("{name}[{}]", list(args, ", ")),
        TypeRefKind::Union(members) => list(members, " | "),
        TypeRefKind::Tuple(elements) if elements.len() == 1 => {
            format!("({},)", render(&elements[0]))
        }
        TypeRefKind::Tuple(elements) => format!("({})", list(elements, ", ")),
        TypeRefKind::Callable {
            task,
            call_kind,
            signature,
        } => {
            let kind = match call_kind {
                ReceiverKind::Borrow => "",
                ReceiverKind::BorrowMut => "mut ",
                ReceiverKind::Value => "own ",
            };
            let name = if *task { "TaskCallable" } else { "Callable" };
            format!("{name}[{kind}{}]", render(signature))
        }
        TypeRefKind::Function {
            params,
            return_type,
            ..
        } => {
            let mut parts = Vec::new();
            let mut keyword_only = false;
            for param in params {
                if param.keyword_only && !keyword_only {
                    keyword_only = true;
                    parts.push("*".to_string());
                }
                let mode = match param.mode {
                    ParamMode::Default => "",
                    ParamMode::Own => "own ",
                    ParamMode::BorrowMut => "mut ",
                };
                let ty = render(&param.ty);
                parts.push(match &param.name {
                    Some(name) => format!("{name}: {mode}{ty}"),
                    None => format!("{mode}{ty}"),
                });
            }
            format!("def({}) -> {}", parts.join(", "), render(return_type))
        }
    }
}
