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
    Argument, ClassDecl, ConstantDecl, Expr, ExprKind, FunctionDecl, FunctionTypeParam, Param,
    ParamMode, ReturnStmt, Stmt, TypeAliasDecl, TypeRef, TypeRefKind,
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
    /// The decorated function's `own` parameters.
    owned: Vec<String>,
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
            let reason = std::mem::take(&mut diagnostic.message);
            diagnostic.message = format!(
                "decorator on `{}` must take and return `{}`: {reason}",
                application.function, application.signature
            );
            if reason.contains("would be renamed") {
                diagnostic.help = vec![
                    "a decorator's callable type names each parameter, so the decorated function must use the same names"
                        .to_string(),
                ];
            } else if let Some(owned) = application
                .owned
                .iter()
                .find(|owned| reason.contains(&format!("{owned}: own ")))
            {
                diagnostic.help.push(format!(
                    "`{}` consumes `{owned}`, and a wrapper that calls it again, such as a retry, has no argument left to pass; write an explicit loop that builds a new argument for each attempt",
                    application.function
                ));
            }
        }
        diagnostic
    }
}

/// Rewrites every decorated function in `module`.
pub(super) fn expand(module: &mut Module) -> Result<Applications> {
    let mut applications = Applications::default();
    let mut items = Vec::with_capacity(module.items.len());
    for mut item in std::mem::take(&mut module.items) {
        match item {
            Item::Function(function) if !function.decorators.is_empty() => {
                let (alias, function, constant) = desugar(function, None, &mut applications)?;
                insert_constant(module, constant);
                items.push(Item::TypeAlias(alias));
                items.push(Item::Function(function));
            }
            Item::Class(ref mut class) => {
                expand_methods(class, module, &mut items, &mut applications)?;
                items.push(item);
            }
            _ => items.push(item),
        }
    }
    module.items = items;
    Ok(applications)
}

fn insert_constant(module: &mut Module, constant: ConstantDecl) {
    let position = module
        .constants
        .iter()
        .position(|existing| {
            (existing.span.line, existing.span.column) > (constant.span.line, constant.span.column)
        })
        .unwrap_or(module.constants.len());
    module.constants.insert(position, constant);
}

/// Keep the source method as the receiver-aware entry point. Its body calls
/// the same kind of initialized constant as a decorated free function. The
/// original body becomes a private associated function so module
/// initialization can pack it without capturing any particular receiver.
fn expand_methods(
    class: &mut ClassDecl,
    module: &mut Module,
    items: &mut Vec<Item>,
    applications: &mut Applications,
) -> Result<()> {
    let mut bodies = Vec::new();
    for method in class
        .methods
        .iter_mut()
        .filter(|m| !m.decorators.is_empty())
    {
        let span = method.span;
        if !class.type_params.is_empty() {
            return Err(Diagnostic::coded_at(
                CODE,
                method.decorators[0].span,
                format!(
                    "decorated method `{}.{}` cannot belong to a generic class",
                    class.name, method.name
                ),
            ));
        }
        if method.name == "__init__" {
            return Err(Diagnostic::coded_at(CODE, method.decorators[0].span,
                "an initializer cannot be decorated because its receiver is still under construction"));
        }
        let mut body = method.clone();
        let mut args = Vec::new();
        if let Some(receiver) = body.receiver.take() {
            body.params.insert(
                0,
                Param {
                    name: "self".into(),
                    mode: match receiver {
                        ReceiverKind::Borrow => ParamMode::Default,
                        ReceiverKind::BorrowMut => ParamMode::BorrowMut,
                        ReceiverKind::Value => ParamMode::Own,
                    },
                    ty: TypeRef::named(&class.name, Vec::new(), false, span),
                    default: None,
                    keyword_only: false,
                    span,
                },
            );
        }
        for p in &body.params {
            args.push(Argument {
                name: p.keyword_only.then(|| p.name.clone()),
                value: Expr {
                    kind: ExprKind::Name(p.name.clone()),
                    span: p.span,
                },
                span: p.span,
            });
        }
        let (alias, mut body, constant) = desugar(body, Some(&class.name), applications)?;
        method.body = vec![Stmt::Return(ReturnStmt {
            value: Some(Expr {
                kind: ExprKind::Call {
                    callee: Box::new(Expr {
                        kind: ExprKind::Name(constant.name.clone()),
                        span,
                    }),
                    args,
                },
                span,
            }),
            view: None,
            span,
        })];
        body.decorators.clear();
        body.doc = None;
        bodies.push(body);
        items.push(Item::TypeAlias(alias));
        insert_constant(module, constant);
    }
    class.methods.extend(bodies);
    Ok(())
}

/// The module-level callable signature needs the class name wherever the
/// method's signature spells its enclosing type as `Self`.
fn resolve_method_self(ty: &mut TypeRef, class: &str) {
    match &mut ty.kind {
        TypeRefKind::Named { name, args } => {
            if name == "Self" {
                *name = class.to_owned();
            }
            for arg in args {
                resolve_method_self(arg, class);
            }
        }
        TypeRefKind::Union(items) | TypeRefKind::Tuple(items) => {
            for item in items {
                resolve_method_self(item, class);
            }
        }
        TypeRefKind::Callable { signature, .. } => resolve_method_self(signature, class),
        TypeRefKind::Function {
            params,
            return_type,
            ..
        } => {
            for param in params {
                resolve_method_self(&mut param.ty, class);
            }
            resolve_method_self(return_type, class);
        }
    }
}

fn desugar(
    mut function: FunctionDecl,
    class: Option<&str>,
    applications: &mut Applications,
) -> Result<(TypeAliasDecl, FunctionDecl, ConstantDecl)> {
    let first = function.decorators[0].span;
    let reject = |message: String| Err(Diagnostic::coded_at(CODE, first, message));
    let name = match class {
        Some(class) => format!("{class}.{}", function.name),
        None => function.name.clone(),
    };
    // Include the class name's length so underscores in source identifiers
    // cannot give two distinct methods the same module-level binding.
    let key = match class {
        Some(class) => format!("method_{}_{}_{}", class.len(), class, function.name),
        None => function.name.clone(),
    };
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
    let mut callable = TypeRef::callable(false, ReceiverKind::Borrow, signature, span);
    if let Some(class) = class {
        resolve_method_self(&mut callable, class);
    }
    let alias = TypeAliasDecl {
        public: false,
        name: format!("{DECORATED_PREFIX}{key}"),
        type_params: Vec::new(),
        type_param_bounds: Default::default(),
        target: callable.clone(),
        span,
        contracts: Vec::new(),
    };

    let undecorated = format!("{UNDECORATED_PREFIX}{}", function.name);
    let target = match class {
        Some(class) => ExprKind::Member {
            object: Box::new(Expr {
                kind: ExprKind::Name(class.to_owned()),
                span,
            }),
            field: undecorated.clone(),
        },
        None => ExprKind::Name(undecorated.clone()),
    };
    // The packed function is the innermost decorator's argument, so a
    // mismatch in passing it reports at that decorator.
    let innermost = function.decorators[function.decorators.len() - 1].span;
    let mut value = call(
        Expr {
            kind: ExprKind::Name(alias.name.clone()),
            span: innermost,
        },
        Expr {
            kind: target,
            span: innermost,
        },
        innermost,
    );
    let owned = function
        .params
        .iter()
        .filter(|param| param.mode == ParamMode::Own)
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    let signature = render(&callable);
    // The undecorated function keeps its decorators for hover and docs.
    for decorator in function.decorators.iter().rev() {
        applications.applications.push(Application {
            span: decorator.span,
            function: name.clone(),
            signature: signature.clone(),
            owned: owned.clone(),
        });
        value = call(decorator.expr.clone(), value, decorator.span);
    }
    let constant = ConstantDecl {
        placement: None,
        mutable: false,
        public: class.is_none() && function.public,
        name: if class.is_some() {
            format!("{DECORATED_PREFIX}slot_{key}")
        } else {
            name
        },
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

#[cfg(test)]
mod method_tests {
    use crate::{check_source, parse_source, run_source};

    #[test]
    fn method_decorators_preserve_receiver_modes_recursion_and_call_order() {
        let source = include_str!("../../tests/fixtures/run-pass/method_decorators.au");
        let expected = include_str!("../../tests/fixtures/run-pass/method_decorators.stdout");
        assert_eq!(run_source(source).unwrap().stdout, expected);
        check_source(include_str!(
            "../../tests/fixtures/check-pass/method_decorators.au"
        ))
        .unwrap();
        let module = parse_source(include_str!(
            "../../tests/fixtures/parse-pass/method_decorators.au"
        ))
        .unwrap();
        let crate::ast::Item::Class(class) = &module.items[0] else {
            panic!("class")
        };
        let method = &class.methods[0];
        assert_eq!(method.decorators[0].text, "@tools.traced");
        assert_eq!(method.decorators[1].text, "@retry(3)");
        assert!(method.public);
    }

    #[test]
    fn method_decorator_restrictions_name_the_method() {
        let error = check_source(include_str!(
            "../../tests/fixtures/check-fail/method_decorator_default.au"
        ))
        .unwrap_err();
        assert_eq!(error.code, "AU2047");
        assert!(error.message.contains("Counter.read"));
    }

    #[test]
    fn decorated_method_admits_only_a_complete_callable() {
        for (source, reason) in [
            (
                "class Box[T]:\n    @wrapped\n    def get(self):\n        pass\n",
                "generic class",
            ),
            (
                "class Box:\n    @wrapped\n    def __init__(self):\n        pass\n",
                "under construction",
            ),
            (
                "class Box:\n    @wrapped\n    def get[T](self, other: T):\n        pass\n",
                "cannot be generic",
            ),
            (
                "class Box:\n    @wrapped\n    field: int64\n",
                "method definition",
            ),
        ] {
            let error = check_source(source).expect_err("invalid decorated declaration");
            assert_eq!(error.code, "AU2047");
            assert!(error.message.contains(reason), "{error}");
        }
        for source in [
            include_str!("../../tests/fixtures/check-fail/property_not_a_name.au"),
            include_str!("../../tests/fixtures/check-fail/property_other_decorator.au"),
        ] {
            parse_source(source).unwrap();
            assert!(check_source(source).is_err());
        }
    }

    #[test]
    fn method_decorator_mismatch_prints_the_complete_unbound_signature() {
        let source = r#"type Empty = Callable[def() -> None]

def keep(f: own Empty) -> Empty:
    return f

class Pipeline:
    @keep
    def run(mut self, visit: Callable[mut def((int64,)) -> None], finish: Callable[own def(own str) -> None]):
        pass
"#;
        let error = check_source(source).unwrap_err();
        assert_eq!(error.code, "AU2002");
        assert!(
            error.message.starts_with(concat!(
            "decorator on `Pipeline.run` must take and return `Callable[def(self: mut Pipeline, ",
            "visit: Callable[mut def((int64,)) -> None], ",
            "finish: Callable[own def(own str) -> None]) -> None]`: ",
        )),
            "{}",
            error.message
        );
    }

    #[test]
    fn decorated_associated_method_resolves_nested_self_types_and_docs() {
        let source = r#"type Group = Callable[def(items: list[(Box, Box | None)]) -> Box]
def keep(f: own Group) -> Group:
    return f

public class Box:
    @keep
    public def first(items: list[(Self, Self | None)]) -> Self:
        return Box()

def main():
    box = Box.first([])
"#;
        let program = check_source(source).unwrap();
        let api = crate::analysis::api_module_from_program("example", &program);
        let class = api
            .declarations
            .iter()
            .find(|entry| entry.name == "Box")
            .unwrap();
        assert_eq!(class.methods.len(), 1);
        assert!(class.methods[0].signature.starts_with("@keep\ndef first("));
    }

    #[test]
    fn documented_method_decorator_example_runs() {
        let source = include_str!("../../../../examples/classes/method_decorators.au");
        assert_eq!(run_source(source).unwrap().stdout, "42\n");
    }

    #[test]
    fn imported_decorated_method_qualifies_its_receiver_in_the_initializer() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass");
        let result = crate::run_path(&fixtures.join("imported_method_decorator.au")).unwrap();
        assert_eq!(
            result.stdout,
            include_str!("../../tests/fixtures/run-pass/imported_method_decorator.stdout")
        );
    }
}
