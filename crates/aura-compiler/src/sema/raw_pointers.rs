//! Type and capability checks for first-class pointers.
use super::*;

pub(super) fn pointer_type(
    args: Vec<Type>,
    names: &TypeDefinitions,
    span: crate::diag::Span,
) -> Result<Type> {
    names.uses_raw_pointer.set(true);
    fn eligible(ty: &Type, names: &TypeDefinitions) -> bool {
        ffi_scalar_type(ty)
            || matches!(ty,Type::Named(name,args) if args.is_empty() && (name == "int128" || name == "uint128"))
            || ty
                .as_fixed_array()
                .is_some_and(|(element, _)| eligible(element, names))
            || matches!(ty, Type::Named(name, _) if names.c_class_names.contains(name))
    }
    match args.as_slice() {
        [ty] if eligible(ty, names) => Ok(Type::Named("ptr".into(), args)),
        _ => Err(Diagnostic::coded_at(
            "AU2083",
            span,
            "ptr[T] needs a scalar, a C class or a fixed array of those types",
        )),
    }
}

/// These point to inline data with no compatible legacy list view.
pub(super) fn requires_pointer_argument(ty: &Type) -> bool {
    ty.as_fixed_array().is_some() || matches!(ty, Type::Named(name,args) if args.is_empty() && matches!(name.as_str(), "int128" | "uint128"))
}

pub(super) fn cast(source: &Type, target: Type, span: crate::diag::Span) -> Result<Type> {
    let address = Type::named("uint64");
    if (source == &address || source.as_raw_pointer().is_some())
        && (target == address || target.as_raw_pointer().is_some())
    {
        Ok(target)
    } else {
        Err(Diagnostic::coded_at("AU2083", span, format!("cannot cast `{source}` to `{target}`; a pointer casts to another pointer or uint64")))
    }
}

pub(crate) fn raw_pointer_result(element: &Type, method: &str) -> Option<Type> {
    match method {
        "offset" => Some(Type::Named("ptr".into(), vec![element.clone()])),
        "read" | "volatile_read" => Some(element.clone()),
        "write" | "volatile_write" => Some(Type::Unit),
        _ => None,
    }
}

pub(crate) fn raw_pointer_parameter(element: &Type, method: &str) -> Option<(&'static str, Type)> {
    match method {
        "offset" => Some(("count", Type::named("int64"))),
        "write" | "volatile_write" => Some(("value", element.clone())),
        _ => None,
    }
}

impl FunctionChecker<'_> {
    pub(super) fn check_raw_pointer_call(
        &self,
        element: &Type,
        method: &str,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let result = raw_pointer_result(element, method).ok_or_else(|| {
            Diagnostic::coded_at(
                "AU2083",
                span,
                format!("ptr[{element}] has no method `{method}`"),
            )
        })?;
        let parameter = super::raw_pointer_parameter(element, method);
        let params = parameter
            .into_iter()
            .map(|(name, ty)| FunctionParamContract {
                name: name.to_owned(),
                ty,
                passing: ReceiverKind::Borrow,
                has_default: false,
                keyword_only: false,
            })
            .collect::<Vec<_>>();
        self.type_check_function_value_args(&params, &result, args, span, locals, None)
    }
}

impl FunctionChecker<'_> {
    pub(super) fn check_raw_address(
        &self,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let [arg] = args else {
            return Err(Diagnostic::coded_at(
                "AU2083",
                span,
                "mem.address_of takes one addressable place",
            ));
        };
        let place = grouped_expr(&arg.value);
        let state = self.is_state_place(place, locals);
        let valid = match &place.kind {
            ExprKind::Name(_) => state,
            ExprKind::Index { object, .. } => self
                .type_of_expr(object, locals)?
                .as_fixed_array()
                .is_some(),
            ExprKind::Member { object, field } => {
                self.imported_state(object, field).is_some()
                    || matches!(self.type_of_member_object_expr(object, locals)?, Type::Named(name, _)
                        if self.resolve_class_info(&name).is_some_and(|class|
                            class.decl.c_layout && class.fields.contains_key(field) && !class.decl.layout_controls.bit_widths.contains_key(field)))
            }
            _ => false,
        };
        if !valid || self.borrow_call_place(place).is_none() {
            return Err(Diagnostic::coded_at("AU3032", span,
                "mem.address_of requires module storage, a fixed element or a C field in a stable place"));
        }
        if !state {
            self.require_mutable_receiver(place, "mem.address_of", span, locals)?;
        }
        let element = self.type_of_expr(place, locals)?;
        pointer_type(vec![element], self.type_names, span)
    }
}

impl ExternFunctionInfo {
    /// An extern ptr[T] accepts both the historical list view and a typed
    /// address. Only the former borrows or writes back an Aura container.
    pub(crate) fn with_raw_arguments<'a>(
        &'a self,
        args: &[Argument],
        span: crate::diag::Span,
        mut infer: impl FnMut(&Expr) -> Option<Type>,
    ) -> Result<std::borrow::Cow<'a, Self>> {
        let mut function = std::borrow::Cow::Borrowed(self);
        if !self.decl.pointer_params.iter().any(|&pointer| pointer) {
            return Ok(function);
        }
        let ordered = bind_call_arguments(
            &self.decl.name,
            &callable_params_from_decl(&self.decl.params),
            args,
            span,
            CallConvention::PositionalOrNamed,
        )?;
        for (i, slot) in ordered.into_iter().enumerate() {
            if !self.decl.pointer_param(i) {
                continue;
            }
            let Some(argument) = slot else {
                continue;
            };
            let Some(ty) = infer(&argument.value) else {
                continue;
            };
            let Type::Named(_, elements) = &self.signature.params[i] else { continue };
            let Some(element) = ty.as_raw_pointer() else {
                if elements.first().is_some_and(requires_pointer_argument) {
                    return Err(Diagnostic::coded_at("AU2083",argument.span,"this C parameter requires a typed raw pointer"));
                }
                continue;
            };
            if elements.first() != Some(element) {
                return Err(Diagnostic::coded_at(
                    "AU2083",
                    argument.span,
                    format!("pointer element `{element}` does not match the declared C parameter"),
                ));
            }
            if self.contract_decl.is_some() {
                return Err(Diagnostic::coded_at(
                    "AU2083",
                    argument.span,
                    "this extern contract requires an Aura list view with a known length",
                ));
            }
            let adapted = function.to_mut();
            adapted.decl.pointer_params[i] = false;
            adapted.decl.params[i].mode = ParamMode::Default;
            adapted.signature.params[i] = ty;
            adapted.signature.param_passings[i] = ReceiverKind::Borrow;
            adapted.c_struct_params[i] = None;
        }
        Ok(function)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str) -> Result<Program> {
        crate::check_module_with_builtin_imports(crate::parse_source(source)?)
    }

    #[test]
    fn typed_pointer_check_fail_fixtures_match_their_diagnostics() {
        for (source, path, expected) in [
            (
                include_str!("../../tests/fixtures/check-fail/raw_pointer_element.au"),
                "crates/aura-compiler/tests/fixtures/check-fail/raw_pointer_element.au",
                include_str!("../../tests/fixtures/check-fail/raw_pointer_element.diag"),
            ),
            (
                include_str!("../../tests/fixtures/check-fail/raw_pointer_permission.au"),
                "crates/aura-compiler/tests/fixtures/check-fail/raw_pointer_permission.au",
                include_str!("../../tests/fixtures/check-fail/raw_pointer_permission.diag"),
            ),
        ] {
            let error = crate::check_source(source).expect_err("invalid pointer operation");
            assert_eq!(error.render_with_source(path, source), expected.trim_end());
        }
    }

    #[test]
    fn typed_pointer_operations_enforce_element_identity_and_addressability() {
        let prefix = "import mem\nextern \"C\" class Pair:\n    first: int32\n    second: int16\n";
        for (args, body, code) in [
            ("p: ptr[int32]", "return p as int16", "AU2083"),
            ("p: int16", "return p as ptr[int32]", "AU2083"),
            ("p: ptr[int32]", "return p.unknown()", "AU2083"),
            ("p: ptr[int32]", "return p == (0 as ptr[int16])", "AU2083"),
            ("p: ptr[int32]", "return p < p", "AU2083"),
            ("p: int16", "return mem.address_of(p)", "AU3032"),
            (
                "p: int16",
                "return mem.address_of(Pair(first=0, second=1).first)",
                "AU3032",
            ),
            ("p: int16", "return mem.address_of()", "AU2083"),
        ] {
            let source = format!("{prefix}\ndef operation({args}) -> bool:\n    {body}\n");
            assert_eq!(check(&source).unwrap_err().code, code, "{body}");
        }
        let mismatch = "extern \"C\" def consume(pointer: ptr[int32]) -> None\n\ndef operation(p: ptr[int8]) -> None:\n    consume(p)\n";
        assert_eq!(check(mismatch).unwrap_err().code, "AU2083");
        let pointer = Type::Named("ptr".into(), vec![Type::named("int16")]);
        assert_eq!(pointer.as_raw_pointer(), Some(&Type::named("int16")));
        assert_eq!(
            raw_pointer_result(&Type::named("int16"), "offset"),
            Some(pointer)
        );
        assert!(raw_pointer_result(&Type::Unit, "unknown").is_none());
    }
}
