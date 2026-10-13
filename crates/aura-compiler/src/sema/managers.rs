//! Typed context managers (ADR-0060, design checkpoint F1-F5).
//!
//! A class is a manager by shape: it declares `__exit__(mut self)` returning
//! `None` or `Result[None, E]`, optionally `__enter__(mut self)` returning
//! `None` or a view of `self`. A class that declares `close(mut self) ->
//! None` and neither dunder is a manager whose entry is empty and whose exit
//! is `close`. Builtin resources keep their documented cleanup. The checker
//! and MIR lowering share this classification so both read one contract.

use super::*;

/// How a `with` item enters its manager after the header value is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ManagerEntry {
    /// No `__enter__`: the stored value is the entered manager.
    None,
    /// `__enter__(mut self) -> None` runs before the body.
    Unit,
    /// `__enter__(mut self) -> view [mut] T from self`: the item binding is
    /// the returned view and the manager lives in a hidden managed binding.
    View { mutable: bool },
}

/// How a `with` item leaves its manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ManagerExit {
    /// A builtin resource's documented cleanup.
    Builtin,
    /// A `close(mut self) -> None` class without dunder methods.
    Close,
    /// `__exit__(mut self) -> None`.
    Unit,
    /// `__exit__(mut self) -> Result[None, E]`, holding the substituted `E`.
    Result(Type),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagerShape {
    pub entry: ManagerEntry,
    pub exit: ManagerExit,
}

/// Why a type is not a usable manager. Each variant renders one `AU2017`
/// message template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ManagerShapeError {
    NotAClass(Type),
    NotAManager(Type),
    EntryShape(Type),
    EntryResult(Type),
}

impl ManagerShapeError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NotAClass(ty) => {
                format!("`with` requires a context manager, found `{ty}`")
            }
            Self::NotAManager(ty) => format!(
                "class `{ty}` is not a context manager: `__exit__(mut self)` is missing or has the wrong shape"
            ),
            Self::EntryShape(ty) => format!(
                "entry of `{ty}` must be declared as `__enter__(mut self)` with no other parameters"
            ),
            Self::EntryResult(ty) => {
                format!("entry of `{ty}` must return `None` or a view of `self`")
            }
        }
    }
}

/// The compiler-named managed binding that holds a manager whose `__enter__`
/// returns a view. `#` cannot appear in a source identifier, so programs
/// cannot name it.
pub(crate) fn hidden_manager_binding(span: crate::diag::Span) -> String {
    format!("with#{}_{}", span.line, span.column)
}

/// Rewrites a body diagnostic that names a hidden managed binding so the
/// reader sees the `with` item it belongs to instead of the internal name.
pub(crate) fn describe_hidden_manager(
    mut diagnostic: Diagnostic,
    span: crate::diag::Span,
) -> Diagnostic {
    let hidden = format!("`{}`", hidden_manager_binding(span));
    let readable = format!(
        "the manager of the `with` item at {}:{}",
        span.line, span.column
    );
    diagnostic.message = diagnostic.message.replace(&hidden, &readable);
    for secondary in &mut diagnostic.secondary_spans {
        secondary.label = secondary.label.replace(&hidden, &readable);
    }
    for note in &mut diagnostic.notes {
        *note = note.replace(&hidden, &readable);
    }
    for help in &mut diagnostic.help {
        *help = help.replace(&hidden, &readable);
    }
    diagnostic
}

/// The synthesized `binding.method()` call that enters or exits a manager.
pub(crate) fn manager_method_call(binding: &str, method: &str, span: crate::diag::Span) -> Expr {
    let name = Expr {
        kind: ExprKind::Name(binding.to_string()),
        span,
    };
    let callee = Expr {
        kind: ExprKind::Member {
            object: Box::new(name),
            field: method.to_string(),
        },
        span,
    };
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(callee),
            args: Vec::new(),
        },
        span,
    }
}

fn method_is_plain_mut_receiver(method: &MethodInfo) -> bool {
    method.decl.receiver == Some(ReceiverKind::BorrowMut)
        && method.signature.params.is_empty()
        && method.decl.type_params.is_empty()
}

/// Classifies `ty` as a manager. `class_info` is the class named by `ty`
/// when it is a user or builtin class visible at the `with` statement.
pub(crate) fn classify_manager(
    ty: &Type,
    class_info: Option<&ClassInfo>,
) -> std::result::Result<ManagerShape, ManagerShapeError> {
    let Type::Named(name, args) = ty else {
        return Err(ManagerShapeError::NotAClass(ty.clone()));
    };
    if is_builtin_io_resource_type(name, args) {
        return Ok(ManagerShape {
            entry: ManagerEntry::None,
            exit: ManagerExit::Builtin,
        });
    }
    let Some(class_info) = class_info else {
        return Err(ManagerShapeError::NotAClass(ty.clone()));
    };
    let substitutions = class_info
        .decl
        .type_params
        .iter()
        .cloned()
        .zip(args.iter().cloned())
        .collect::<HashMap<_, _>>();

    let Some(exit) = class_info.methods.get("__exit__") else {
        let close_only = class_info.methods.get("close").is_some_and(|close| {
            method_is_plain_mut_receiver(close)
                && close.signature.return_type == Type::Unit
                && close.decl.view_return.is_none()
        });
        if close_only && !class_info.methods.contains_key("__enter__") {
            return Ok(ManagerShape {
                entry: ManagerEntry::None,
                exit: ManagerExit::Close,
            });
        }
        return Err(ManagerShapeError::NotAManager(ty.clone()));
    };
    if !method_is_plain_mut_receiver(exit) || exit.decl.view_return.is_some() {
        return Err(ManagerShapeError::NotAManager(ty.clone()));
    }
    let exit = match substitute_type(&exit.signature.return_type, &substitutions) {
        Type::Unit => ManagerExit::Unit,
        Type::Named(result, result_args)
            if result == "Result" && result_args.len() == 2 && result_args[0] == Type::Unit =>
        {
            ManagerExit::Result(result_args[1].clone())
        }
        _ => return Err(ManagerShapeError::NotAManager(ty.clone())),
    };

    let entry = match class_info.methods.get("__enter__") {
        None => ManagerEntry::None,
        Some(enter) => {
            if !method_is_plain_mut_receiver(enter) {
                return Err(ManagerShapeError::EntryShape(ty.clone()));
            }
            match &enter.decl.view_return {
                Some(view) if view.origin == "self" => ManagerEntry::View {
                    mutable: view.mutable,
                },
                Some(_) => return Err(ManagerShapeError::EntryResult(ty.clone())),
                None if enter.signature.return_type == Type::Unit => ManagerEntry::Unit,
                None => return Err(ManagerShapeError::EntryResult(ty.clone())),
            }
        }
    };
    Ok(ManagerShape { entry, exit })
}

impl FunctionChecker<'_> {
    /// Checks the manager protocol of a `with` item's value and, for a
    /// fallible exit, that the enclosing function can propagate the exit
    /// error as `try` would.
    pub(super) fn require_with_manager(
        &self,
        value_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<ManagerShape> {
        let class_info = match value_ty {
            Type::Named(name, _) => self.classes.get(name),
            _ => None,
        };
        let shape = classify_manager(value_ty, class_info)
            .map_err(|error| Diagnostic::coded_at("AU2017", span, error.message()))?;
        if let ManagerExit::Result(error_ty) = &shape.exit {
            let compatible = match &self.current_return_type {
                Some(Type::Named(name, args)) if name == "Result" && args.len() == 2 => {
                    args[1] == *error_ty || self.has_from_conversion(error_ty, &args[1], span)?
                }
                _ => false,
            };
            if !compatible {
                return Err(Diagnostic::coded_at(
                    "AU2017",
                    span,
                    format!(
                        "exit of `{value_ty}` may fail with `{error_ty}`; the enclosing function must return `Result[_, {error_ty}]`"
                    ),
                ));
            }
        }
        Ok(shape)
    }
}
