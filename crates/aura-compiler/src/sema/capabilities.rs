//! Declaration-stable ownership, argument acquisition, and capability checks.

use super::{
    Argument, BuiltinFunction, BuiltinMember, ClosureCaptureMode, Diagnostic, Expr, ExprKind,
    FunctionChecker, HashMap, LocalBinding, Param, ParamMode, PlacePath, ReceiverKind, Result,
    RngCloneSafety, Type,
};

/// Whether a checked assertion dispatch can retain both operands for failure
/// reporting without changing its ownership semantics.
///
/// `None` denotes compiler-defined comparison or membership dispatch. Custom
/// operator dispatch supplies its resolved receiver and right-operand
/// conventions; both must be shared. In particular, mutable or consuming
/// contracts remain ordinary assertions and are never cloned for diagnostics.
pub(crate) fn assertion_dispatch_is_non_consuming(
    custom_passings: Option<(ReceiverKind, ReceiverKind)>,
) -> bool {
    custom_passings.is_none_or(|(receiver, rhs)| {
        receiver == ReceiverKind::Borrow && rhs == ReceiverKind::Borrow
    })
}

/// Maps a written parameter capability to its passing convention.
///
/// ADR-0022 Q1 ratifies universal logical sharing: bare means shared access
/// for every type, including declaration-known copy types. The ABI may still
/// pass copied bits, but the source-level shared-loan and ADR-0016 sequencing
/// rules apply uniformly. That is what keeps this mapping declaration-stable,
/// which generic trait specializations and builtin signatures depend on.
pub(crate) fn resolve_param_passing(mode: ParamMode) -> ReceiverKind {
    match mode {
        ParamMode::Default => ReceiverKind::Borrow,
        ParamMode::Own => ReceiverKind::Value,
        ParamMode::BorrowMut => ReceiverKind::BorrowMut,
    }
}

pub(super) fn resolve_param_passings(params: &[Param]) -> Vec<ReceiverKind> {
    params
        .iter()
        .map(|param| resolve_param_passing(param.mode))
        .collect()
}

impl<'a> FunctionChecker<'a> {
    pub(super) fn require_transfer(
        &self,
        ty: &Type,
        context: impl Into<String>,
        span: crate::diag::Span,
    ) -> Result<()> {
        let Some(reason) = self.transfer_failure(ty) else {
            return Ok(());
        };
        Err(Diagnostic::coded_at(
            "AU3008",
            span,
            format!(
                "{} cannot cross a task boundary because {reason}",
                context.into()
            ),
        )
        .with_help(
            "send owned data made only from Transfer components; keep capabilities and host resources on their owning worker",
        ))
    }

    pub(super) fn require_queue_payload_transfer(
        &self,
        payload_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let Some(reason) = self.transfer_failure(payload_ty) else {
            return Ok(());
        };
        Err(Diagnostic::coded_at(
            "AU3008",
            span,
            format!(
                "Queue payload `{payload_ty}` cannot cross a worker boundary because {reason}"
            ),
        )
        .with_help(
            "use a Queue payload made only from Transfer components; keep capabilities and host resources on their owning worker",
        ))
    }

    pub(super) fn consume_task_observation_right(
        &self,
        task: &Expr,
        result_ty: &Type,
        _operation: &str,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let task_ty = Type::Named("Task".to_string(), vec![result_ty.clone()]);
        if self.is_copy_type(&task_ty) {
            return Ok(());
        }
        self.consume_value_expr(task, locals)
    }

    pub(super) fn consume_task_collection_observation_right(
        &self,
        tasks: &Expr,
        result_ty: &Type,
        _operation: &str,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let task_ty = Type::Named("Task".to_string(), vec![result_ty.clone()]);
        if self.is_copy_type(&task_ty) {
            return Ok(());
        }
        self.consume_value_expr(tasks, locals)
    }

    pub(super) fn apply_builtin_argument_passing(
        &self,
        member: BuiltinMember,
        index: usize,
        argument: &Argument,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let passing = member
            .argument_passing(index)
            .expect("type-checked builtin argument must have passing metadata");
        self.apply_operator_operand_passing(
            &argument.value,
            passing,
            &format!("builtin method `{}` argument", member.name()),
            locals,
        )
    }

    pub(super) fn apply_builtin_function_argument_passing(
        &self,
        builtin: BuiltinFunction,
        index: usize,
        argument: &Argument,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let passing = builtin
            .argument_passing(index)
            .expect("type-checked builtin argument must have passing metadata");
        self.apply_operator_operand_passing(
            &argument.value,
            passing,
            &format!("builtin function `{}` argument", builtin.name()),
            locals,
        )
    }

    pub(super) fn apply_operator_operand_passing(
        &self,
        expr: &Expr,
        passing: ReceiverKind,
        label: &str,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.reject_view_value_for_passing(expr, passing, label, locals)?;
        match passing {
            ReceiverKind::Value => {
                if let Some(place) = self.borrow_call_place(expr) {
                    self.ensure_place_not_shared_by_match_for_move(&place, expr.span, locals)?;
                }
                self.consume_value_expr(expr, locals)
            }
            ReceiverKind::Borrow => Ok(()),
            ReceiverKind::BorrowMut => {
                if !self.is_mutable_place(expr, locals)? {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        expr.span,
                        format!("{} is declared `mut` and requires a mutable place", label),
                    ));
                }
                if let Some(place) = self.borrow_call_place(expr) {
                    self.ensure_place_mutation_allowed(&place, expr.span, locals)?;
                    self.invalidate_narrowing(
                        &place,
                        expr.span,
                        "a call with mutable access",
                        locals,
                    );
                }
                Ok(())
            }
        }
    }

    pub(super) fn consume_binding(
        &self,
        name: &str,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let binding = locals
            .get(name)
            .ok_or_else(|| Diagnostic::at(span, format!("unknown name `{}`", name)))?;
        if self.is_copy_type(&binding.ty) {
            return Ok(());
        }
        let place = PlacePath::root(name.to_string());
        // A read narrowed to one Copy member copies that payload instead of
        // consuming the union (ADR-0052 A3); a non-Copy member still moves
        // the whole union.
        if !self.suppress_narrowing.get() {
            if let Some(fact) = binding.narrowed.get(&place.projections) {
                if let [member] = fact.members.as_slice() {
                    if self.is_copy_type(member) {
                        return Ok(());
                    }
                }
            }
        }
        self.ensure_place_not_frozen_for_move(&place, span, locals)?;
        self.ensure_place_not_locked_by_view(&place, None, span, locals)?;
        let binding = locals
            .get_mut(name)
            .ok_or_else(|| Diagnostic::at(span, format!("unknown name `{}`", name)))?;
        let duplication_member = self.builtin_duplication_member(&binding.ty);
        let clone_supported = duplication_member.is_some();
        if binding.passing != ReceiverKind::Value {
            if binding
                .borrow_origin
                .as_deref()
                .is_some_and(|origin| origin.starts_with("module constant `"))
            {
                return Err(Diagnostic::coded_at(
                    "AU3001",
                    span,
                    format!("cannot move `{name}` out of immutable module storage"),
                )
                .with_help(if let Some(member) = duplication_member {
                    format!("call `.{member}()` when an independent owned value is required")
                } else {
                    "keep shared access or construct an independent owned value".to_string()
                }));
            }
            if let Some(ty) = self.implicit_borrowed_params.get(name) {
                let mut diagnostic = Diagnostic::at(
                    span,
                    if clone_supported {
                        format!(
                            "parameter `{}` is borrowed; declare it as `own {}` to take ownership, or {} the value before consuming it",
                            name,
                            ty,
                            if duplication_member == Some("copy") { "copy" } else { "clone" }
                        )
                    } else {
                        format!(
                            "parameter `{}` is borrowed; declare it as `own {}` to take ownership",
                            name, ty
                        )
                    },
                );
                if let Some(origin) = binding.borrowed_at {
                    diagnostic = diagnostic
                        .with_secondary(origin, format!("parameter `{}` is borrowed here", name));
                }
                diagnostic = diagnostic.with_help(if clone_supported {
                    format!(
                        "declare the parameter as `own {}` when the function should consume it, or call `.{duplication_member}()` to consume an independent copy",
                        ty,
                        duplication_member = duplication_member.expect("clone-supported values have a duplication member")
                    )
                } else {
                    format!(
                        "declare the parameter as `own {}` when the function should consume this non-cloneable value",
                        ty
                    )
                });
                if clone_supported {
                    let insertion = crate::diag::Span::new(
                        span.line,
                        span.column.saturating_add(name.chars().count()),
                    );
                    diagnostic = diagnostic.with_edit(
                        insertion,
                        insertion,
                        format!(
                            ".{}()",
                            duplication_member
                                .expect("clone-supported values have a duplication member")
                        ),
                    );
                }
                return Err(diagnostic);
            }
            // ADR-0022 Q2: a payload bound by a bare match has exactly one
            // replacement, so name it instead of the generic borrowed-move
            // guidance that would send the caller looking for a parameter.
            if let Some(scrutinee) = &binding.shared_match_scrutinee {
                let mut diagnostic = Diagnostic::coded_at(
                    "AU3002",
                    span,
                    format!("cannot move `{name}` out of a shared match on `{scrutinee}`"),
                );
                if let Some(origin) = binding.borrowed_at {
                    diagnostic = diagnostic.with_secondary(origin, "value is borrowed here");
                }
                diagnostic = diagnostic.with_help(if clone_supported {
                    format!(
                        "write `match own {scrutinee}` to consume the scrutinee, or call `.{duplication_member}()` to consume an independent copy",
                        duplication_member = duplication_member.expect("clone-supported values have a duplication member")
                    )
                } else {
                    format!(
                        "write `match own {scrutinee}` to consume the scrutinee; `{}` cannot be cloned",
                        binding.ty
                    )
                });
                if clone_supported {
                    let insertion = crate::diag::Span::new(
                        span.line,
                        span.column.saturating_add(name.chars().count()),
                    );
                    diagnostic = diagnostic.with_edit(
                        insertion,
                        insertion,
                        format!(
                            ".{}()",
                            duplication_member
                                .expect("clone-supported values have a duplication member")
                        ),
                    );
                }
                return Err(diagnostic);
            }
            let mut diagnostic =
                Diagnostic::at(span, format!("cannot move borrowed value `{}`", name));
            if let Some(origin) = binding.borrowed_at {
                diagnostic = diagnostic.with_secondary(origin, "value is borrowed here");
            }
            diagnostic = diagnostic.with_help(if clone_supported {
                format!(
                    "take `{}` as `own {}` when ownership is required, or call `.{duplication_member}()` to consume an independent copy",
                    name,
                    binding.ty,
                    duplication_member = duplication_member.expect("clone-supported values have a duplication member")
                )
            } else {
                format!(
                    "take `{}` as `own {}` when ownership of this non-cloneable value is required",
                    name, binding.ty
                )
            });
            if clone_supported {
                let insertion = crate::diag::Span::new(
                    span.line,
                    span.column.saturating_add(name.chars().count()),
                );
                diagnostic = diagnostic.with_edit(
                    insertion,
                    insertion,
                    format!(
                        ".{}()",
                        duplication_member
                            .expect("clone-supported values have a duplication member")
                    ),
                );
            }
            return Err(diagnostic);
        }
        if binding.managed_resource {
            return Err(Diagnostic::at(
                span,
                format!("cannot move managed `with` resource `{}`", name),
            ));
        }
        if binding.moved {
            return Err(self.moved_value_diagnostic(name, span, binding));
        }
        binding.moved = true;
        binding.moved_at = Some(span);
        self.invalidate_match_borrow_bindings_for_place(&PlacePath::root(name.to_string()), locals);
        Ok(())
    }

    pub(super) fn builtin_duplication_member(&self, ty: &Type) -> Option<&'static str> {
        if let Type::Union(union) = ty {
            // A union clones when every member is Copy or clones itself.
            return union
                .members
                .iter()
                .all(|member| {
                    self.is_copy_type(member) || self.builtin_duplication_member(member).is_some()
                })
                .then_some("clone");
        }
        let Type::Named(name, _) = ty else {
            return None;
        };
        let member = if matches!(name.as_str(), "list" | "dict" | "set") {
            "copy"
        } else {
            "clone"
        };
        (BuiltinMember::resolve(name, member).is_some()
            && self.rng_clone_safety(ty) == RngCloneSafety::Safe
            && self.nonrepeatable_task_result_in(ty).is_none())
        .then_some(member)
    }

    pub(super) fn moved_value_diagnostic(
        &self,
        name: &str,
        span: crate::diag::Span,
        binding: &LocalBinding,
    ) -> Diagnostic {
        let mut diagnostic = Diagnostic::at(span, format!("use of moved value `{}`", name));
        if let Some(origin) = binding.moved_at {
            // A bound method's synthesized receiver capture records the move
            // at the receiver expression, so the origin label names it.
            let closure_move = self.closure_infos.borrow().values().find_map(|closure| {
                closure
                    .captures
                    .iter()
                    .find(|capture| {
                        capture.mode == ClosureCaptureMode::Move
                            && capture.span == origin
                            && (capture.name == name
                                || capture.name == super::BOUND_RECEIVER_CAPTURE)
                    })
                    .map(|capture| capture.name == super::BOUND_RECEIVER_CAPTURE)
            });
            diagnostic = diagnostic.with_secondary(
                origin,
                match closure_move {
                    Some(true) => "value moved into bound method here",
                    Some(false) => "value moved into closure here",
                    None => "value moved here",
                },
            );
            if let Some(duplication_member) = self.builtin_duplication_member(&binding.ty) {
                diagnostic = diagnostic.with_help(format!(
                    "pass shared access when ownership is not needed, or call `.{duplication_member}()` at the move site when an independent value is required"
                ));
                let insertion = crate::diag::Span::new(
                    origin.line,
                    origin.column.saturating_add(name.chars().count()),
                );
                diagnostic =
                    diagnostic.with_edit(insertion, insertion, format!(".{duplication_member}()"));
            } else {
                diagnostic = diagnostic.with_help(
                    "pass shared access when ownership is not needed, or transfer this non-cloneable value only once",
                );
            }
        }
        diagnostic
    }

    pub(super) fn consume_value_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        // Every path through this helper consumes an owned value. Keep the
        // region check centralized so calls, projections, constants, `try`,
        // owned control-flow scrutinees, builtin arguments, and operator
        // operands cannot each grow a separate view-erasure loophole.
        self.reject_owned_view_value(expr, locals, "an owned value")?;
        let key = expr as *const Expr as usize;
        let Some(entry) = self.expr_result_entries.borrow_mut().remove(&key) else {
            return self.consume_value_expr_raw(expr, locals);
        };

        // Type checking and ownership transfer are separate at most call
        // sites. Replaying from the state immediately before this expression
        // lets owned result transfer happen in source order with moves caused
        // while evaluating the expression itself. Isolate entries created by
        // replay so they can serve nested owned arguments without reusing or
        // leaking entries from the original type-check pass.
        let post_typecheck = locals.clone();
        let mut replay_locals = entry.locals;
        let saved_entries = std::mem::take(&mut *self.expr_result_entries.borrow_mut());
        let replay_result =
            self.type_expr_consuming_result(expr, &mut replay_locals, entry.expected.as_ref());
        *self.expr_result_entries.borrow_mut() = saved_entries;
        replay_result?;
        self.merge_control_flow_moves(locals, &[&post_typecheck, &replay_locals]);
        Ok(())
    }

    /// Consumes an ordinary value produced in a read-only context without
    /// mistaking a direct view read for ownership transfer of its descriptor.
    /// Capability validation runs before this helper; mutable returned
    /// temporaries therefore remain rejected while shared/local views are
    /// read through their pointee for the duration of this expression.
    pub(super) fn consume_direct_read_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if self.direct_view_value_kind(expr, locals)?.is_some() {
            return Ok(());
        }
        self.consume_value_expr(expr, locals)
    }

    pub(super) fn consume_value_expr_raw(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => Ok(()),
            ExprKind::Name(name) if self.resolve_function_info(name).is_some() => {
                // A named function expression is an immediate Copy code
                // pointer, not a local binding that can be moved.
                Ok(())
            }
            ExprKind::Name(name) => self.consume_binding(name, expr.span, locals),
            ExprKind::Group(inner) => self.consume_value_expr_raw(inner, locals),
            ExprKind::Specialize { expr, .. } => self.consume_value_expr_raw(expr, locals),
            ExprKind::Member { object, field } => {
                if self.is_payload_free_variant_expr(expr) {
                    return Ok(());
                }
                // A bound method moved its receiver while it was typed, and an
                // associated method value is an immediate Copy code pointer.
                if self.bound_method_closure_at(expr.span).is_some()
                    || self
                        .associated_method_target(object, field, locals)
                        .is_some()
                {
                    return Ok(());
                }
                if let Some((module_path, function_name)) = self.qualified_module_item(expr) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if namespace.constants.contains_key(&function_name) {
                            let rendered = format!("{module_path}.{function_name}");
                            return Err(Diagnostic::coded_at(
                                "AU3001",
                                expr.span,
                                format!("cannot move `{rendered}` out of immutable module storage"),
                            )
                            .with_help(
                                "keep shared access or construct an independent owned value",
                            ));
                        }
                        if namespace.functions.contains_key(&function_name)
                            || namespace.all_functions.contains_key(&function_name)
                        {
                            // A module-qualified function value is an immediate
                            // Copy code pointer, not a member borrowed from a
                            // runtime module object.
                            return Ok(());
                        }
                    }
                }
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                let member_ty = self.resolve_member_type(&object_ty, field, expr.span)?;
                self.consume_typed_member_value_expr(
                    expr, object, field, &object_ty, &member_ty, locals,
                )
            }
            // `receiver.method[T]` moved its receiver while it was typed.
            ExprKind::Index { object, .. }
                if matches!(object.kind, ExprKind::Member { .. })
                    && self.bound_method_closure_at(object.span).is_some() =>
            {
                Ok(())
            }
            ExprKind::Index { .. } => {
                if let Some(place) = self.borrow_call_place(expr) {
                    if self
                        .place_path_type(&place, locals, expr.span)
                        .ok()
                        .flatten()
                        .is_some_and(|ty| !self.is_copy_type(&ty))
                    {
                        self.ensure_place_not_locked_by_view(&place, None, expr.span, locals)?;
                    }
                }
                self.type_of_expr(expr, locals).map(|_| ())
            }
            // Slices materialize a fresh owned value. Their source and
            // endpoints were fully checked while producing that value; moving
            // the result must not replay or consume any of those inputs.
            ExprKind::Slice { .. } => Ok(()),
            // Composite, branching, and fallible results have one branch-aware
            // walk. Reaching it from here means the expression is consumed
            // without a recorded pre-expression state, so the walk starts from
            // the current state instead of a replayed one.
            _ if Self::result_consumption_needs_replay(expr) => self
                .type_expr_consuming_result(expr, locals, None)
                .map(|_| ()),
            _ => Ok(()),
        }
    }

    pub(super) fn consume_typed_member_value_expr(
        &self,
        expr: &Expr,
        object: &Expr,
        field: &str,
        object_ty: &Type,
        member_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if self.is_copy_type(member_ty) {
            return Ok(());
        }
        if let Some(place) = self.borrow_call_place(expr) {
            self.ensure_place_not_locked_by_view(&place, None, expr.span, locals)?;
        }
        if let Some(name) = self.borrowed_root_binding_name(object, locals) {
            let mut diagnostic = Diagnostic::at(
                expr.span,
                format!(
                    "cannot move non-copy field `{}` out of borrowed value `{}`",
                    field, name
                ),
            );
            if let Some(origin) = locals.get(&name).and_then(|binding| binding.borrowed_at) {
                diagnostic =
                    diagnostic.with_secondary(origin, format!("`{}` is borrowed here", name));
            }
            let duplication_member = self.builtin_duplication_member(member_ty);
            let clone_supported = duplication_member.is_some();
            diagnostic = diagnostic.with_help(if clone_supported {
                format!(
                    "take `{}` as `own {}` when the field should be moved, or call `.{duplication_member}()` on the field to return an independent value",
                    name,
                    object_ty,
                    duplication_member = duplication_member.expect("clone-supported values have a duplication member")
                )
            } else {
                format!(
                    "take `{}` as `own {}` when this non-cloneable field should be moved",
                    name, object_ty
                )
            });
            if clone_supported {
                let insertion = crate::diag::Span::new(
                    expr.span.line,
                    expr.span.column.saturating_add(field.chars().count()),
                );
                diagnostic = diagnostic.with_edit(
                    insertion,
                    insertion,
                    format!(
                        ".{}()",
                        duplication_member
                            .expect("clone-supported values have a duplication member")
                    ),
                );
            }
            return Err(diagnostic);
        }
        if let Some(path) = self.member_access_path(expr) {
            self.ensure_place_not_frozen_for_move(&path, expr.span, locals)?;
            if let Some(binding) = locals.get_mut(&path.root) {
                if binding.managed_resource {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "cannot move non-copy field `{}` out of managed `with` resource `{}`",
                            field, path.root
                        ),
                    ));
                }
                binding
                    .moved_fields
                    .insert(path.projections.clone(), expr.span);
            }
            self.invalidate_match_borrow_bindings_for_place(&path, locals);
        }
        Ok(())
    }

    pub(super) fn consume_match_scrutinee_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let ungrouped = match &expr.kind {
            ExprKind::Group(inner) => inner.as_ref(),
            _ => expr,
        };
        if let ExprKind::Member { object, field } = &ungrouped.kind {
            let object_ty = self.type_of_member_object_expr(object, locals)?;
            let member_ty = self.resolve_member_type(&object_ty, field, ungrouped.span)?;
            if !self.is_copy_type(&member_ty) {
                if let Some(root) = self.borrowed_root_binding_name(object, locals) {
                    let rendered_place = self.render_place_expr(ungrouped);
                    let mut diagnostic = Diagnostic::coded_at(
                        "AU3002",
                        ungrouped.span,
                        format!(
                            "cannot move non-copy field `{}` out of borrowed value `{}` in match scrutinee; use `match {}:` to inspect it by shared access",
                            field,
                            root,
                            rendered_place
                        ),
                    );
                    if let Some(origin) = locals.get(&root).and_then(|binding| binding.borrowed_at)
                    {
                        diagnostic = diagnostic
                            .with_secondary(origin, format!("`{}` is borrowed here", root));
                    }
                    diagnostic = diagnostic.with_help(format!(
                        "use `match {}:` to inspect the field without moving it",
                        rendered_place
                    ));
                    return Err(diagnostic);
                }
            }
        }
        self.consume_value_expr(expr, locals)
    }

    pub(super) fn capability_name(passing: ReceiverKind) -> &'static str {
        match passing {
            ReceiverKind::Borrow => "shared",
            ReceiverKind::BorrowMut => "mut",
            ReceiverKind::Value => "own",
        }
    }

    pub(super) fn is_mutable_place(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<bool> {
        match &expr.kind {
            ExprKind::Name(name) => {
                if let Some(binding) = locals.get(name).filter(|binding| binding.captured) {
                    // An owned capture is the closure's own state: mutating it
                    // makes the closure Mutable and keeps the updated value in
                    // its environment (C2). A shared-view capture stays
                    // read-only.
                    if binding.passing == ReceiverKind::Value {
                        self.mutated_captures.borrow_mut().insert(name.clone());
                        return Ok(true);
                    }
                    return Err(Diagnostic::coded_at(
                        "AU3003",
                        expr.span,
                        format!(
                            "lambda capture `{name}` is a shared view and cannot be mutably accessed"
                        ),
                    )
                    .with_help(format!(
                        "capture `mut {name}` for a mutable view, or move an owned value into the lambda to mutate closure-owned state"
                    )));
                }
                Ok(locals
                    .get(name)
                    .map(|binding| binding.mutable_place)
                    .unwrap_or(false))
            }
            ExprKind::Group(inner) => self.is_mutable_place(inner, locals),
            ExprKind::Member { object, field } => {
                self.resolve_member_target_type(object, field, expr.span, locals)?;
                self.is_mutable_place(object, locals)
            }
            ExprKind::Index { object, index } => {
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                let Type::Tuple(elements) = object_ty else {
                    return Ok(false);
                };
                let ExprKind::Int(value) = index.kind else {
                    return Ok(false);
                };
                let Ok(index) = usize::try_from(value) else {
                    return Ok(false);
                };
                if index >= elements.len() {
                    return Ok(false);
                }
                self.is_mutable_place(object, locals)
            }
            ExprKind::Call { .. } => {
                let mutable_result = self.returned_view_call_kind(expr, locals)?
                    == Some(crate::ast::ViewKind::Mutable);
                if !mutable_result {
                    return Ok(false);
                }
                Ok(self.view_place(expr, locals)?.is_some())
            }
            _ => Ok(false),
        }
    }

    pub(super) fn require_mutable_receiver(
        &self,
        object: &Expr,
        method_name: &str,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if let Some(place) = self.borrow_call_place(object) {
            self.ensure_place_mutation_allowed(&place, span, locals)?;
            let through_view = locals
                .get(&place.root)
                .and_then(|binding| binding.view.as_ref())
                .map(|_| place.root.as_str());
            self.ensure_place_not_locked_by_view(&place, through_view, span, locals)?;
            self.invalidate_narrowing(&place, span, "a mutating method call", locals);
        }
        if self.is_mutable_place(object, locals)? {
            return Ok(());
        }
        if self.is_shared_self_place(object, locals) {
            return Err(self.shared_self_mutation_diagnostic(span, locals));
        }
        if let Some(place) = self.borrow_call_place(object) {
            if locals
                .get(&place.root)
                .and_then(|binding| binding.borrow_origin.as_deref())
                .is_some_and(|origin| origin.starts_with("module constant `"))
            {
                return Err(Diagnostic::coded_at(
                    "AU3003",
                    span,
                    format!(
                        "module constant `{}` cannot provide a mutable receiver for method `{method_name}`",
                        place.root
                    ),
                )
                .with_help(
                    "put mutable state in a local value owned by `main` or another explicit owner",
                ));
            }
        }
        Err(Diagnostic::coded_at(
            "AU3003",
            span,
            format!("method `{}` requires a mutable receiver", method_name),
        )
        .with_help("declare the receiver place with `mut` before calling a mutating method"))
    }

    pub(super) fn is_shared_self_place(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> bool {
        self.member_access_path(expr)
            .filter(|place| place.root == "self")
            .and_then(|_| locals.get("self"))
            .is_some_and(|binding| binding.passing == ReceiverKind::Borrow)
    }

    pub(super) fn shared_self_mutation_diagnostic(
        &self,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Diagnostic {
        let mut diagnostic = Diagnostic::coded_at(
            "AU3003",
            span,
            "cannot mutate through shared receiver `self`; declare the receiver as `mut self`",
        );
        if let Some(origin) = locals.get("self").and_then(|binding| binding.borrowed_at) {
            diagnostic =
                diagnostic.with_secondary(origin, "shared receiver `self` is declared here");
        }
        diagnostic
            .with_help("declare the receiver as `mut self` when the method mutates through `self`")
    }

    pub(super) fn borrowed_root_binding_name(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(name) => locals
                .get(name)
                .filter(|binding| binding.passing != ReceiverKind::Value)
                .map(|_| name.clone()),
            ExprKind::Group(inner) => self.borrowed_root_binding_name(inner, locals),
            ExprKind::Member { object, .. } => self.borrowed_root_binding_name(object, locals),
            ExprKind::Index { object, .. } => self.borrowed_root_binding_name(object, locals),
            _ => None,
        }
    }
}
