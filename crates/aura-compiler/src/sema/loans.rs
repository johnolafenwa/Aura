//! Loan provenance, returned-view footprints, last use, reborrows, and access conflicts.

use super::{
    bind_call_arguments, block_references_name, callable_params_from_decl, expr_references_name,
    grouped_expr, grouped_specialized_expr, resolve_param_passing, statement_span,
    stmt_references_name, substitute_type, substitutions_from_decl_type_args, unify_type_pattern,
    Argument, AssignTarget, BTreeMap, BTreeSet, BuiltinAssociatedFunction, BuiltinMember,
    CallConvention, ClassInfo, ClosureOwner, ComprehensionOutput, Diagnostic, Expr, ExprKind,
    FunctionChecker, FunctionDecl, FunctionInfo, FunctionSignature, HashMap, LocalBinding,
    MatchStmt, MethodInfo, ParamMode, PlacePath, ProjectedField, ReceiverKind, Result, Stmt,
    TraitBound, TraitInfo, Type,
};

pub(super) fn view_return_contract_key(decl: &FunctionDecl) -> Option<(bool, bool, usize)> {
    let contract = decl.view_return.as_ref()?;
    if contract.origin == "self" {
        return Some((contract.mutable, true, 0));
    }
    let origin_index = decl
        .params
        .iter()
        .position(|param| param.name == contract.origin)
        .unwrap_or(usize::MAX);
    Some((contract.mutable, false, origin_index))
}

pub(super) fn later_span(
    current: Option<crate::diag::Span>,
    candidate: Option<crate::diag::Span>,
) -> Option<crate::diag::Span> {
    match (current, candidate) {
        (Some(left), Some(right)) if span_precedes(left, right) => Some(right),
        (Some(left), _) => Some(left),
        (None, right) => right,
    }
}

pub(super) fn last_name_reference_span_in_block(
    body: &[Stmt],
    name: &str,
) -> Option<crate::diag::Span> {
    body.iter()
        .rev()
        .find_map(|stmt| last_name_reference_span_in_stmt(stmt, name))
}

pub(super) fn last_name_reference_span_in_match(
    match_stmt: &MatchStmt,
    name: &str,
) -> Option<crate::diag::Span> {
    let mut last =
        expr_references_name(&match_stmt.scrutinee, name).then_some(match_stmt.scrutinee.span);
    for arm in &match_stmt.arms {
        last = later_span(
            last,
            arm.guard
                .as_ref()
                .filter(|guard| expr_references_name(guard, name))
                .map(|guard| guard.span),
        );
        last = later_span(last, last_name_reference_span_in_block(&arm.body, name));
    }
    last
}

/// Returns a span at the actual nested use rather than the header of the
/// enclosing control-flow statement. Loan expiration compares source
/// positions, so collapsing an inner use to an earlier `if`/`match`/loop
/// header would otherwise end an inherited loan before checking that body.
pub(super) fn last_name_reference_span_in_stmt(
    stmt: &Stmt,
    name: &str,
) -> Option<crate::diag::Span> {
    if !stmt_references_name(stmt, name) {
        return None;
    }
    match stmt {
        Stmt::If(if_stmt) => {
            let mut last = None;
            for branch in &if_stmt.branches {
                last = later_span(
                    last,
                    expr_references_name(&branch.condition, name).then_some(branch.condition.span),
                );
                last = later_span(last, last_name_reference_span_in_block(&branch.body, name));
            }
            if let Some(body) = &if_stmt.else_body {
                last = later_span(last, last_name_reference_span_in_block(body, name));
            }
            last.or(Some(if_stmt.span))
        }
        Stmt::Match(match_stmt) => {
            last_name_reference_span_in_match(match_stmt, name).or(Some(match_stmt.span))
        }
        Stmt::For(for_stmt) => Some(
            last_name_reference_span_in_block(&for_stmt.body, name)
                .map(|span| later_span(Some(for_stmt.span), Some(span)).unwrap_or(for_stmt.span))
                .or_else(|| last_name_reference_span_in_expr(&for_stmt.iterable, name))
                .unwrap_or(for_stmt.span),
        ),
        Stmt::With(with_stmt) => later_span(
            expr_references_name(&with_stmt.value, name).then_some(with_stmt.value.span),
            last_name_reference_span_in_block(&with_stmt.body, name),
        )
        .or(Some(with_stmt.span)),
        Stmt::While(while_stmt) => Some(
            block_end_span(&while_stmt.body)
                .map(|span| {
                    later_span(Some(while_stmt.span), Some(span)).unwrap_or(while_stmt.span)
                })
                .unwrap_or(while_stmt.span),
        ),
        Stmt::Assign(assign) => {
            last_name_reference_span_in_expr(&assign.value, name).or_else(|| match &assign.target {
                AssignTarget::Name(target) if target == name => Some(assign.span),
                AssignTarget::Member { object, .. } => {
                    last_name_reference_span_in_expr(object, name)
                }
                AssignTarget::Index { object, index } => later_span(
                    last_name_reference_span_in_expr(object, name),
                    last_name_reference_span_in_expr(index, name),
                ),
                _ => None,
            })
        }
        Stmt::View(view) => last_name_reference_span_in_expr(&view.source, name),
        Stmt::Destructure(stmt) => last_name_reference_span_in_expr(&stmt.value, name),
        Stmt::Assert(stmt) => later_span(
            last_name_reference_span_in_expr(&stmt.condition, name),
            stmt.message
                .as_ref()
                .and_then(|message| last_name_reference_span_in_expr(message, name)),
        ),
        Stmt::Return(stmt) => stmt
            .value
            .as_ref()
            .and_then(|value| last_name_reference_span_in_expr(value, name)),
        Stmt::Expr(stmt) => last_name_reference_span_in_expr(&stmt.expr, name),
        Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => None,
    }
}

pub(super) fn block_end_span(body: &[Stmt]) -> Option<crate::diag::Span> {
    body.last().map(statement_end_span)
}

pub(super) fn statement_end_span(stmt: &Stmt) -> crate::diag::Span {
    match stmt {
        Stmt::If(stmt) => stmt
            .else_body
            .as_ref()
            .and_then(|body| block_end_span(body))
            .or_else(|| {
                stmt.branches
                    .last()
                    .and_then(|branch| block_end_span(&branch.body))
            })
            .unwrap_or(stmt.span),
        Stmt::Match(stmt) => stmt
            .arms
            .last()
            .and_then(|arm| block_end_span(&arm.body))
            .unwrap_or(stmt.span),
        Stmt::For(stmt) => block_end_span(&stmt.body).unwrap_or(stmt.span),
        Stmt::With(stmt) => block_end_span(&stmt.body).unwrap_or(stmt.span),
        Stmt::While(stmt) => block_end_span(&stmt.body).unwrap_or(stmt.span),
        _ => statement_span(stmt),
    }
}

pub(super) fn last_name_reference_span_in_expr(
    expr: &Expr,
    name: &str,
) -> Option<crate::diag::Span> {
    let combine = |items: Vec<Option<crate::diag::Span>>| items.into_iter().fold(None, later_span);
    match &expr.kind {
        ExprKind::Name(candidate) => (candidate == name).then_some(expr.span),
        ExprKind::Group(inner)
        | ExprKind::Try(inner)
        | ExprKind::Specialize { expr: inner, .. }
        | ExprKind::Unary { expr: inner, .. }
        | ExprKind::IsNone { value: inner, .. }
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::Member { object: inner, .. } => last_name_reference_span_in_expr(inner, name),
        ExprKind::Index { object, index } => combine(vec![
            last_name_reference_span_in_expr(object, name),
            last_name_reference_span_in_expr(index, name),
        ]),
        ExprKind::Slice {
            object, start, end, ..
        } => combine(vec![
            last_name_reference_span_in_expr(object, name),
            start
                .as_deref()
                .and_then(|value| last_name_reference_span_in_expr(value, name)),
            end.as_deref()
                .and_then(|value| last_name_reference_span_in_expr(value, name)),
        ]),
        ExprKind::Call { callee, args } => combine(
            std::iter::once(last_name_reference_span_in_expr(callee, name))
                .chain(
                    args.iter()
                        .map(|argument| last_name_reference_span_in_expr(&argument.value, name)),
                )
                .collect(),
        ),
        ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => combine(
            elements
                .iter()
                .map(|element| last_name_reference_span_in_expr(element, name))
                .collect(),
        ),
        ExprKind::Map(entries) => combine(
            entries
                .iter()
                .flat_map(|entry| [&entry.key, &entry.value])
                .map(|value| last_name_reference_span_in_expr(value, name))
                .collect(),
        ),
        ExprKind::Comprehension { output, clauses } => {
            let mut spans = clauses
                .iter()
                .flat_map(|clause| std::iter::once(&clause.iterable).chain(clause.filters.iter()))
                .map(|value| last_name_reference_span_in_expr(value, name))
                .collect::<Vec<_>>();
            match output {
                ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                    spans.push(last_name_reference_span_in_expr(value, name));
                }
                ComprehensionOutput::Map { key, value } => {
                    spans.push(last_name_reference_span_in_expr(key, name));
                    spans.push(last_name_reference_span_in_expr(value, name));
                }
            }
            combine(spans)
        }
        ExprKind::FString(parts) => combine(
            parts
                .iter()
                .filter_map(|part| match part {
                    crate::ast::FormatPart::Literal(_) => None,
                    crate::ast::FormatPart::Expr(value)
                    | crate::ast::FormatPart::Formatted { expr: value, .. } => {
                        Some(last_name_reference_span_in_expr(value, name))
                    }
                })
                .collect(),
        ),
        ExprKind::Match {
            scrutinee, arms, ..
        } => combine(
            std::iter::once(last_name_reference_span_in_expr(scrutinee, name))
                .chain(
                    arms.iter()
                        .map(|arm| last_name_reference_span_in_expr(&arm.value, name)),
                )
                .collect(),
        ),
        ExprKind::Membership {
            value, container, ..
        }
        | ExprKind::Binary {
            left: value,
            right: container,
            ..
        } => combine(vec![
            last_name_reference_span_in_expr(value, name),
            last_name_reference_span_in_expr(container, name),
        ]),
        ExprKind::CompareChain { first, links } => combine(
            std::iter::once(last_name_reference_span_in_expr(first, name))
                .chain(
                    links
                        .iter()
                        .map(|link| last_name_reference_span_in_expr(&link.operand, name)),
                )
                .collect(),
        ),
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => combine(vec![
            last_name_reference_span_in_expr(condition, name),
            last_name_reference_span_in_expr(then_expr, name),
            last_name_reference_span_in_expr(else_expr, name),
        ]),
        ExprKind::Lambda { params, body, .. } if params.iter().any(|param| param.name == name) => {
            None
        }
        ExprKind::Lambda { body, .. } => last_name_reference_span_in_expr(body, name),
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted => None,
    }
}

pub(super) fn span_precedes(left: crate::diag::Span, right: crate::diag::Span) -> bool {
    (left.line, left.column) < (right.line, right.column)
}

pub(super) fn collect_view_return_entries(
    body: &[Stmt],
    aliases: &mut BTreeMap<String, Expr>,
    values: &mut Vec<(Expr, BTreeMap<String, Expr>)>,
) {
    for stmt in body {
        match stmt {
            Stmt::View(view) => {
                aliases.insert(view.name.clone(), view.source.clone());
            }
            Stmt::Return(stmt) if stmt.view.is_some() => {
                if let Some(value) = &stmt.value {
                    values.push((value.clone(), aliases.clone()));
                }
            }
            Stmt::If(stmt) => {
                for branch in &stmt.branches {
                    collect_view_return_entries(&branch.body, &mut aliases.clone(), values);
                }
                if let Some(body) = &stmt.else_body {
                    collect_view_return_entries(body, &mut aliases.clone(), values);
                }
            }
            Stmt::Match(stmt) => {
                for arm in &stmt.arms {
                    collect_view_return_entries(&arm.body, &mut aliases.clone(), values);
                }
            }
            Stmt::For(stmt) => {
                collect_view_return_entries(&stmt.body, &mut aliases.clone(), values)
            }
            Stmt::With(stmt) => {
                collect_view_return_entries(&stmt.body, &mut aliases.clone(), values)
            }
            Stmt::While(stmt) => {
                collect_view_return_entries(&stmt.body, &mut aliases.clone(), values)
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
pub(super) enum ReturnedProjectionSummary {
    Known(BTreeSet<String>),
    Cycle,
    Unknown,
}

#[derive(Default)]
pub(super) struct ReturnedViewSummaryContext {
    pub(super) locals: BTreeMap<String, Type>,
    pub(super) type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

#[derive(Clone)]
pub(super) struct ViewBinding {
    pub(super) kind: crate::ast::ViewKind,
    pub(super) source: PlacePath,
    /// The immediately reborrowed view, when this loan was created through
    /// another descriptor.  Ancestors are suspended while a descendant is
    /// live and resume after the descendant's inferred last use.
    pub(super) parent: Option<String>,
    /// Every transitive parent captured when the reborrow is created.  This
    /// lineage must outlive an intermediate parent's active loan metadata:
    /// an intermediate view can reach its inferred last use while a deeper
    /// descendant still keeps the original ancestor suspended.
    pub(super) ancestors: BTreeSet<String>,
    pub(super) created_at: crate::diag::Span,
    pub(super) last_use: crate::diag::Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BorrowSourceInfo {
    pub(super) origin: String,
    pub(super) passing: ReceiverKind,
    pub(super) match_borrow_place: Option<PlacePath>,
    pub(super) shared_match_scrutinee: Option<String>,
}

#[derive(Clone)]
pub(super) struct BorrowedCallPlace {
    pub(super) path: PlacePath,
    pub(super) passing: ReceiverKind,
    pub(super) param_name: String,
    pub(super) origin_span: crate::diag::Span,
}

#[derive(Clone)]
pub(super) struct ActiveMatchBorrow {
    /// Canonical physical place protected by the mutable match.
    pub(super) physical: PlacePath,
    /// Syntactic scrutinee route authorized to update that place. Re-spelling
    /// the same place through an owner or a different view remains forbidden.
    pub(super) access: PlacePath,
}

impl<'a> FunctionChecker<'a> {
    pub(super) fn ensure_pattern_binding_not_stale(
        &self,
        name: &str,
        span: crate::diag::Span,
        binding: &LocalBinding,
    ) -> Result<()> {
        if let Some(place) = &binding.stale_match_borrow_place {
            if binding.shared_match_scrutinee.is_some() {
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    span,
                    format!(
                        "cannot use shared match binding `{name}` after changing match scrutinee `{place}`"
                    ),
                )
                .with_help(format!(
                    "finish using `{name}` before changing `{place}`; use `match mut {place}` to update its payload or `match own {place}` to consume it"
                )));
            }
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "cannot use pattern binding `{}` after reassigning match scrutinee `{}`",
                    name, place
                ),
            ));
        }
        Ok(())
    }

    pub(super) fn invalidate_match_borrow_bindings_for_place(
        &self,
        place: &PlacePath,
        locals: &mut HashMap<String, LocalBinding>,
    ) {
        for binding in locals.values_mut() {
            if binding
                .match_borrow_place
                .as_ref()
                .is_some_and(|binding_place| binding_place.overlaps(place))
            {
                binding.stale_match_borrow_place = binding.match_borrow_place.clone();
            }
        }
    }

    pub(super) fn invalidate_match_borrow_bindings_for_borrowed_places(
        &self,
        places: &[BorrowedCallPlace],
        locals: &mut HashMap<String, LocalBinding>,
    ) {
        for place in places {
            if place.passing == ReceiverKind::BorrowMut {
                self.invalidate_match_borrow_bindings_for_place(&place.path, locals);
            }
        }
    }

    pub(super) fn validate_view_return_contract(&self, decl: &FunctionDecl) -> Result<()> {
        let Some(contract) = &decl.view_return else {
            return Ok(());
        };
        if contract.origin == "self" {
            let Some(receiver) = decl.receiver else {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    contract.span,
                    "`from self` is valid only on a method with a receiver",
                ));
            };
            if receiver == ReceiverKind::Value {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    contract.span,
                    "an owned `self` receiver cannot be the origin of a returned view",
                ));
            }
            if contract.mutable && receiver != ReceiverKind::BorrowMut {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    contract.span,
                    "a mutable returned view requires a `mut self` origin",
                ));
            }
            return Ok(());
        }
        let Some(param) = decl
            .params
            .iter()
            .find(|param| param.name == contract.origin)
        else {
            return Err(Diagnostic::coded_at(
                "AU3010",
                contract.span,
                format!(
                    "returned-view origin `{}` is not a receiver or parameter",
                    contract.origin
                ),
            ));
        };
        if param.mode == ParamMode::Own {
            return Err(Diagnostic::coded_at(
                "AU3010",
                param.span,
                format!(
                    "owned parameter `{}` cannot be the origin of a returned view",
                    param.name
                ),
            ));
        }
        if param.default.is_some() {
            return Err(Diagnostic::coded_at(
                "AU3010",
                param.span,
                format!(
                    "defaulted parameter `{}` cannot be the origin of a returned view",
                    param.name
                ),
            ));
        }
        if contract.mutable && param.mode != ParamMode::BorrowMut {
            return Err(Diagnostic::coded_at(
                "AU3010",
                contract.span,
                format!(
                    "a mutable returned view requires mutable origin parameter `{}`",
                    param.name
                ),
            ));
        }
        Ok(())
    }

    /// The scrutinee spelling to quote in a `match own <place>` suggestion.
    ///
    /// Only a bare (shared) match over a named place can be respelled, so a
    /// temporary scrutinee or an already-explicit capability yields `None`.
    pub(super) fn shared_match_scrutinee_name(
        &self,
        expr: &Expr,
        capability: ReceiverKind,
    ) -> Option<String> {
        if capability != ReceiverKind::Borrow {
            return None;
        }
        matches!(
            expr.kind,
            ExprKind::Name(_) | ExprKind::Member { .. } | ExprKind::Index { .. }
        )
        .then(|| self.render_place_expr(expr))
    }

    /// Returns a conservative source place for a bare shared match. Field
    /// projections retain their precision; indexed scrutinees retain the
    /// collection root because Aura does not yet model index identity.
    pub(super) fn shared_match_place(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<PlacePath> {
        let place = match &expr.kind {
            ExprKind::Name(name) => Some(PlacePath::root(name.clone())),
            ExprKind::Group(inner) => self.shared_match_place(inner, locals),
            ExprKind::Member { object, field } => Some(
                self.shared_match_place(object, locals)?
                    .with_field(field.clone()),
            ),
            ExprKind::Index { object, .. } => self.shared_match_place(object, locals),
            ExprKind::Call { .. } => {
                let mut place_locals = locals.clone();
                self.view_place(expr, &mut place_locals).ok().flatten()
            }
            _ => None,
        };
        place.map(|place| self.canonicalize_view_place(place, locals))
    }

    pub(super) fn retain_shared_match_place(
        &self,
        place: &PlacePath,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) {
        if let Some(binding) = locals.get_mut(&place.root) {
            binding.shared_match_places.insert(place.clone(), span);
        }
    }

    /// Resolves the physical place held for the duration of a borrowing
    /// iteration. Ordinary place expressions use their spelling directly;
    /// returned views use the callee contract to recover the caller place.
    pub(super) fn borrowed_iterable_place(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<PlacePath>> {
        if let Some(place) = self.borrow_call_place(expr) {
            return Ok(Some(self.canonicalize_view_place(place, locals)));
        }
        self.view_place(expr, locals)
    }

    pub(super) fn canonicalize_view_place(
        &self,
        place: PlacePath,
        locals: &HashMap<String, LocalBinding>,
    ) -> PlacePath {
        let mut current = place;
        let mut seen = BTreeSet::new();
        while seen.insert(current.root.clone()) {
            let Some(binding) = locals.get(&current.root) else {
                break;
            };
            let Some(view) = binding.view.as_ref() else {
                break;
            };
            let source_is_conservative_footprint = self
                .place_path_type(&view.source, locals, view.created_at)
                .ok()
                .flatten()
                .is_some_and(|source_ty| source_ty != binding.ty);
            current = if source_is_conservative_footprint {
                // A branch-selected returned view keeps a conservative origin
                // footprint for conflict checks.  Its child projections are
                // typed relative to the alias, but cannot safely be appended
                // to that broader origin path.
                view.source.clone()
            } else {
                view.source.followed_by(&current.projections)
            };
        }
        current
    }

    pub(super) fn view_expr_has_conservative_footprint(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<bool> {
        match &expr.kind {
            ExprKind::Group(inner) => self.view_expr_has_conservative_footprint(inner, locals),
            ExprKind::Name(name) => {
                let Some(binding) = locals.get(name) else {
                    return Ok(false);
                };
                let Some(view) = binding.view.as_ref() else {
                    return Ok(false);
                };
                Ok(self
                    .place_path_type(&view.source, locals, view.created_at)?
                    .is_some_and(|source_ty| source_ty != binding.ty))
            }
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                self.view_expr_has_conservative_footprint(object, locals)
            }
            ExprKind::Call { callee, args } => {
                // Projection summaries with zero or multiple alternatives
                // deliberately identify only the declaration's conservative
                // origin. A projection applied by an outer call or member
                // access must not be appended to that broader place.
                let mut contract_locals = locals.clone();
                let Some((decl, receiver, owner_module)) =
                    self.returned_view_callee(callee, &mut contract_locals)?
                else {
                    return Ok(false);
                };
                if decl.view_return.is_none() {
                    return Ok(false);
                }
                let substitutions = self.returned_view_call_type_substitutions(
                    &decl,
                    &owner_module,
                    receiver.as_ref(),
                    callee,
                    args,
                    &mut contract_locals,
                )?;
                Ok(self
                    .transitive_unique_returned_view_projection(
                        &decl,
                        &owner_module,
                        &mut BTreeSet::new(),
                        &substitutions,
                    )
                    .is_none())
            }
            _ => Ok(false),
        }
    }

    pub(super) fn view_place(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<PlacePath>> {
        let place = match &expr.kind {
            ExprKind::Name(name) => Some(PlacePath::root(name.clone())),
            ExprKind::Group(inner) => return self.view_place(inner, locals),
            ExprKind::Member { object, field } => {
                let conservative = self.view_expr_has_conservative_footprint(object, locals)?;
                self.view_place(object, locals)?.map(|place| {
                    if conservative {
                        place
                    } else {
                        place.with_field(field.clone())
                    }
                })
            }
            ExprKind::Index { object, index } => {
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                let Type::Tuple(elements) = object_ty else {
                    return Err(Diagnostic::coded_at(
                        "AU3004",
                        expr.span,
                        "indexed collection elements do not have stable view identity; only fixed tuple positions are supported",
                    )
                    .with_help("return or store an index, handle, or owned clone instead"));
                };
                let ExprKind::Int(value) = &index.kind else {
                    return Err(Diagnostic::coded_at(
                        "AU3004",
                        index.span,
                        "a tuple view requires a fixed integer position",
                    ));
                };
                let index = usize::try_from(*value).map_err(|_| {
                    Diagnostic::coded_at("AU3004", index.span, "invalid tuple view position")
                })?;
                if index >= elements.len() {
                    return Err(Diagnostic::coded_at(
                        "AU3004",
                        expr.span,
                        format!("tuple has no position {index}"),
                    ));
                }
                let conservative = self.view_expr_has_conservative_footprint(object, locals)?;
                self.view_place(object, locals)?.map(|place| {
                    if conservative {
                        place
                    } else {
                        place.with_tuple(index)
                    }
                })
            }
            ExprKind::Call { callee, args } => {
                return self.returned_view_call_place(callee, args, locals)
            }
            _ => None,
        };
        Ok(place.map(|place| self.canonicalize_view_place(place, locals)))
    }

    pub(super) fn returned_view_callee(
        &self,
        callee: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<(crate::ast::FunctionDecl, Option<Expr>, String)>> {
        let base = grouped_specialized_expr(callee);
        match &base.kind {
            ExprKind::Name(name) => {
                if locals.contains_key(name) {
                    return Ok(None);
                }
                Ok(self
                    .resolve_function_info(name)
                    .map(|function| (function.decl.clone(), None, function.module_name.clone())))
            }
            ExprKind::Member { object, field } => {
                if let Some((module_path, function_name)) = self.qualified_module_item(base) {
                    let function = self.module_namespace(&module_path).and_then(|namespace| {
                        namespace
                            .functions
                            .get(&function_name)
                            .or_else(|| namespace.all_functions.get(&function_name))
                    });
                    return Ok(function.map(|function| {
                        (function.decl.clone(), None, function.module_name.clone())
                    }));
                }
                if let Some((module_path, class_name)) = self.qualified_module_item(object) {
                    let method = self.module_namespace(&module_path).and_then(|namespace| {
                        namespace.classes.get(&class_name).and_then(|class| {
                            class.methods.get(field).map(|method| (class, method))
                        })
                    });
                    return Ok(method
                        .filter(|(_, method)| method.decl.receiver.is_none())
                        .map(|(class, method)| {
                            (method.decl.clone(), None, class.module_name.clone())
                        }));
                }
                let object_base = grouped_specialized_expr(object);
                if let ExprKind::Name(name) = &object_base.kind {
                    if !locals.contains_key(name) {
                        if let Some(class) = self.resolve_class_info(name) {
                            return Ok(class
                                .methods
                                .get(field)
                                .filter(|method| method.decl.receiver.is_none())
                                .map(|method| {
                                    (method.decl.clone(), None, class.module_name.clone())
                                }));
                        }
                        return Ok(None);
                    }
                }
                let receiver_ty = self.type_of_member_object_expr(object, locals)?;
                if let Type::Named(class_name, _) = &receiver_ty {
                    if let Some(class) = self.resolve_class_info(class_name) {
                        if let Some(method) = class.methods.get(field) {
                            return Ok(Some((
                                method.decl.clone(),
                                Some((**object).clone()),
                                class.module_name.clone(),
                            )));
                        }
                    }
                }
                if let Type::TypeParam(type_param_name) = &receiver_ty {
                    if let Ok(method) = self.trait_method_from_type_param(type_param_name, field) {
                        return Ok(Some((
                            method.decl,
                            Some((**object).clone()),
                            method.module_name,
                        )));
                    }
                }
                if let Some((trait_impl, method, _substitutions)) =
                    self.trait_method_for_concrete_type(&receiver_ty, field, callee.span)?
                {
                    return Ok(Some((
                        method.decl.clone(),
                        Some((**object).clone()),
                        trait_impl.module_name.clone(),
                    )));
                }
                Ok(None)
            }
            ExprKind::Index { object, .. } => self.returned_view_callee(object, locals),
            _ => Ok(None),
        }
    }

    pub(super) fn returned_view_call_place(
        &self,
        callee: &Expr,
        args: &[Argument],
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<PlacePath>> {
        let Some((decl, receiver, owner_module)) = self.returned_view_callee(callee, locals)?
        else {
            return Ok(None);
        };
        let Some(contract) = &decl.view_return else {
            return Ok(None);
        };
        let substitutions = self.returned_view_call_type_substitutions(
            &decl,
            &owner_module,
            receiver.as_ref(),
            callee,
            args,
            locals,
        )?;
        if contract.origin == "self" {
            let receiver = receiver.as_ref().ok_or_else(|| {
                Diagnostic::coded_at(
                    "AU3010",
                    callee.span,
                    "returned view origin `self` requires an instance receiver",
                )
            })?;
            let conservative = self.view_expr_has_conservative_footprint(receiver, locals)?;
            return self
                .view_place(receiver, locals)?
                .ok_or_else(|| {
                    Diagnostic::coded_at(
                        "AU3010",
                        receiver.span,
                        "returned view origin `self` requires an addressable receiver place",
                    )
                })
                .map(|mut place| {
                    if !conservative {
                        if let Some(projection) = self.transitive_unique_returned_view_projection(
                            &decl,
                            &owner_module,
                            &mut BTreeSet::new(),
                            &substitutions,
                        ) {
                            for segment in projection.split('.').filter(|part| !part.is_empty()) {
                                place = match segment.parse::<usize>() {
                                    Ok(index) => place.with_tuple(index),
                                    Err(_) => place.with_field(segment.to_string()),
                                };
                            }
                        }
                    }
                    Some(place)
                });
        }
        let Some(origin_index) = decl
            .params
            .iter()
            .position(|param| param.name == contract.origin)
        else {
            return Ok(None);
        };
        let ordered = bind_call_arguments(
            &format!("callable `{}`", decl.name),
            &callable_params_from_decl(&decl.params),
            args,
            callee.span,
            CallConvention::PositionalOrNamed,
        )?;
        let Some(argument) = ordered.get(origin_index).and_then(|argument| *argument) else {
            return Err(Diagnostic::coded_at(
                "AU3010",
                callee.span,
                format!(
                    "returned view origin `{}` must be supplied by an addressable caller argument",
                    contract.origin
                ),
            ));
        };
        let conservative = self.view_expr_has_conservative_footprint(&argument.value, locals)?;
        self.view_place(&argument.value, locals)?
            .ok_or_else(|| {
                Diagnostic::coded_at(
                    "AU3010",
                    argument.value.span,
                    format!(
                        "returned view origin `{}` requires an addressable caller place",
                        contract.origin
                    ),
                )
            })
            .map(|mut place| {
                if !conservative {
                    if let Some(projection) = self.transitive_unique_returned_view_projection(
                        &decl,
                        &owner_module,
                        &mut BTreeSet::new(),
                        &substitutions,
                    ) {
                        for segment in projection.split('.').filter(|part| !part.is_empty()) {
                            place = match segment.parse::<usize>() {
                                Ok(index) => place.with_tuple(index),
                                Err(_) => place.with_field(segment.to_string()),
                            };
                        }
                    }
                }
                Some(place)
            })
    }

    pub(super) fn returned_view_call_type_substitutions(
        &self,
        decl: &crate::ast::FunctionDecl,
        owner_module: &str,
        receiver: Option<&Expr>,
        callee: &Expr,
        args: &[Argument],
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<HashMap<String, Type>> {
        let (_, explicit_type_args) = self.peel_specialization(callee);
        let mut substitutions = if let Some(type_args) = explicit_type_args {
            self.explicit_type_substitutions(
                &decl.type_params,
                type_args,
                callee.span,
                &format!("callable `{}`", decl.name),
            )?
        } else {
            HashMap::new()
        };
        let context = self.returned_view_summary_context(owner_module, decl);
        if let (Some(receiver), Some(pattern)) = (receiver, context.locals.get("self")) {
            let mut type_locals = locals.clone();
            let actual = self.type_of_expr(receiver, &mut type_locals)?;
            let _ = unify_type_pattern(pattern, &actual, &mut substitutions);
        }
        let ordered = bind_call_arguments(
            &format!("callable `{}`", decl.name),
            &callable_params_from_decl(&decl.params),
            args,
            callee.span,
            CallConvention::PositionalOrNamed,
        )?;
        for (param, argument) in decl.params.iter().zip(ordered) {
            let Some(argument) = argument else {
                continue;
            };
            let Some(pattern) = context.locals.get(&param.name) else {
                continue;
            };
            let mut type_locals = locals.clone();
            let actual = self.type_of_expr(&argument.value, &mut type_locals)?;
            let _ = unify_type_pattern(pattern, &actual, &mut substitutions);
        }
        Ok(substitutions)
    }

    pub(super) fn transitive_unique_returned_view_projection(
        &self,
        decl: &crate::ast::FunctionDecl,
        owner_module: &str,
        seen: &mut BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
    ) -> Option<String> {
        let ReturnedProjectionSummary::Known(mut projections) =
            self.returned_view_projection_summary(decl, owner_module, seen, substitutions)
        else {
            return None;
        };
        (projections.len() == 1).then(|| projections.pop_first().expect("one returned projection"))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn returned_view_expr_projection_summary_with_aliases(
        &self,
        owner_module: &str,
        enclosing_decl: &crate::ast::FunctionDecl,
        expr: &Expr,
        outer_origin: &str,
        seen: &mut BTreeSet<String>,
        aliases: &BTreeMap<String, Expr>,
        alias_seen: &mut BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
    ) -> ReturnedProjectionSummary {
        let expr = grouped_expr(expr);
        match &expr.kind {
            ExprKind::Name(name) if name == outer_origin => {
                return ReturnedProjectionSummary::Known(BTreeSet::from([String::new()]));
            }
            ExprKind::Name(name) => {
                let Some(alias) = aliases.get(name) else {
                    return ReturnedProjectionSummary::Unknown;
                };
                if !alias_seen.insert(name.clone()) {
                    return ReturnedProjectionSummary::Unknown;
                }
                let summary = self.returned_view_expr_projection_summary_with_aliases(
                    owner_module,
                    enclosing_decl,
                    alias,
                    outer_origin,
                    seen,
                    aliases,
                    alias_seen,
                    substitutions,
                );
                alias_seen.remove(name);
                return summary;
            }
            ExprKind::Member { object, field } => {
                return match self.returned_view_expr_projection_summary_with_aliases(
                    owner_module,
                    enclosing_decl,
                    object,
                    outer_origin,
                    seen,
                    aliases,
                    alias_seen,
                    substitutions,
                ) {
                    ReturnedProjectionSummary::Known(projections) => {
                        ReturnedProjectionSummary::Known(
                            projections
                                .into_iter()
                                .map(|projection| {
                                    if projection.is_empty() {
                                        field.clone()
                                    } else {
                                        format!("{projection}.{field}")
                                    }
                                })
                                .collect(),
                        )
                    }
                    other => other,
                };
            }
            ExprKind::Index { object, index } => {
                let ExprKind::Int(index) = index.kind else {
                    return ReturnedProjectionSummary::Unknown;
                };
                let Ok(index) = usize::try_from(index) else {
                    return ReturnedProjectionSummary::Unknown;
                };
                return match self.returned_view_expr_projection_summary_with_aliases(
                    owner_module,
                    enclosing_decl,
                    object,
                    outer_origin,
                    seen,
                    aliases,
                    alias_seen,
                    substitutions,
                ) {
                    ReturnedProjectionSummary::Known(projections) => {
                        ReturnedProjectionSummary::Known(
                            projections
                                .into_iter()
                                .map(|projection| {
                                    if projection.is_empty() {
                                        index.to_string()
                                    } else {
                                        format!("{projection}.{index}")
                                    }
                                })
                                .collect(),
                        )
                    }
                    other => other,
                };
            }
            ExprKind::Call { .. } => {}
            _ => return ReturnedProjectionSummary::Unknown,
        }
        let ExprKind::Call { callee, args } = &expr.kind else {
            unreachable!("call arm is selected above")
        };
        let Some((callee_decl, callee_owner)) = self.returned_view_function_in_owner(
            owner_module,
            enclosing_decl,
            callee,
            substitutions,
            aliases,
        ) else {
            return ReturnedProjectionSummary::Unknown;
        };
        let Some(callee_contract) = &callee_decl.view_return else {
            return ReturnedProjectionSummary::Unknown;
        };
        let origin_expr = if callee_contract.origin == "self" {
            let ExprKind::Member { object, .. } = &grouped_specialized_expr(callee).kind else {
                return ReturnedProjectionSummary::Unknown;
            };
            object.as_ref()
        } else {
            let Some(index) = callee_decl
                .params
                .iter()
                .position(|param| param.name == callee_contract.origin)
            else {
                return ReturnedProjectionSummary::Unknown;
            };
            let Ok(ordered) = bind_call_arguments(
                &format!("callable `{}`", callee_decl.name),
                &callable_params_from_decl(&callee_decl.params),
                args,
                callee.span,
                CallConvention::PositionalOrNamed,
            ) else {
                return ReturnedProjectionSummary::Unknown;
            };
            let Some(argument) = ordered.get(index).copied().flatten() else {
                return ReturnedProjectionSummary::Unknown;
            };
            &argument.value
        };
        let bases = self.returned_view_expr_projection_summary_with_aliases(
            owner_module,
            enclosing_decl,
            origin_expr,
            outer_origin,
            seen,
            aliases,
            alias_seen,
            substitutions,
        );
        let nested_substitutions = self.returned_view_nested_call_type_substitutions(
            owner_module,
            enclosing_decl,
            &callee_decl,
            &callee_owner,
            callee,
            args,
            substitutions,
            aliases,
        );
        let nested = self.returned_view_projection_summary(
            &callee_decl,
            &callee_owner,
            seen,
            &nested_substitutions,
        );
        match (bases, nested) {
            (ReturnedProjectionSummary::Known(bases), ReturnedProjectionSummary::Known(nested)) => {
                ReturnedProjectionSummary::Known(
                    bases
                        .into_iter()
                        .flat_map(|base| {
                            nested.iter().cloned().map(move |projection| {
                                match (base.is_empty(), projection.is_empty()) {
                                    (true, _) => projection,
                                    (_, true) => base.clone(),
                                    _ => format!("{base}.{projection}"),
                                }
                            })
                        })
                        .collect(),
                )
            }
            (ReturnedProjectionSummary::Cycle, _) | (_, ReturnedProjectionSummary::Cycle) => {
                ReturnedProjectionSummary::Cycle
            }
            _ => ReturnedProjectionSummary::Unknown,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn returned_view_nested_call_type_substitutions(
        &self,
        owner_module: &str,
        enclosing_decl: &crate::ast::FunctionDecl,
        callee_decl: &crate::ast::FunctionDecl,
        callee_owner: &str,
        callee: &Expr,
        args: &[Argument],
        outer_substitutions: &HashMap<String, Type>,
        aliases: &BTreeMap<String, Expr>,
    ) -> HashMap<String, Type> {
        let mut substitutions = HashMap::new();
        let (_, explicit_type_args) = self.peel_specialization(callee);
        if let Some(type_args) = explicit_type_args {
            if let Ok(lowered) = self.lower_explicit_type_args(type_args) {
                let lowered = lowered
                    .iter()
                    .map(|ty| substitute_type(ty, outer_substitutions))
                    .collect::<Vec<_>>();
                substitutions =
                    substitutions_from_decl_type_args(&callee_decl.type_params, &lowered);
            }
        }

        let enclosing_context = self.returned_view_summary_context(owner_module, enclosing_decl);
        let callee_context = self.returned_view_summary_context(callee_owner, callee_decl);
        if let ExprKind::Member { object, .. } = &grouped_specialized_expr(callee).kind {
            if let (Some(pattern), Some(actual)) = (
                callee_context.locals.get("self"),
                self.returned_view_expr_type_in_owner(
                    owner_module,
                    enclosing_decl,
                    object,
                    &enclosing_context,
                    outer_substitutions,
                    aliases,
                    &mut BTreeSet::new(),
                ),
            ) {
                let _ = unify_type_pattern(pattern, &actual, &mut substitutions);
            }
        }
        let Ok(ordered) = bind_call_arguments(
            &format!("callable `{}`", callee_decl.name),
            &callable_params_from_decl(&callee_decl.params),
            args,
            callee.span,
            CallConvention::PositionalOrNamed,
        ) else {
            return substitutions;
        };
        for (param, argument) in callee_decl.params.iter().zip(ordered) {
            let Some(argument) = argument else {
                continue;
            };
            let (Some(pattern), Some(actual)) = (
                callee_context.locals.get(&param.name),
                self.returned_view_expr_type_in_owner(
                    owner_module,
                    enclosing_decl,
                    &argument.value,
                    &enclosing_context,
                    outer_substitutions,
                    aliases,
                    &mut BTreeSet::new(),
                ),
            ) else {
                continue;
            };
            let _ = unify_type_pattern(pattern, &actual, &mut substitutions);
        }
        substitutions
    }

    pub(super) fn returned_view_projection_summary(
        &self,
        decl: &crate::ast::FunctionDecl,
        owner_module: &str,
        seen: &mut BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
    ) -> ReturnedProjectionSummary {
        let Some(contract) = decl.view_return.as_ref() else {
            return ReturnedProjectionSummary::Unknown;
        };
        let key = format!(
            "{}::{}:{}:{}:{:?}:{:?}:{substitutions:?}",
            owner_module, decl.name, decl.span.line, decl.span.column, decl.view_return, decl.body
        );
        if !seen.insert(key.clone()) {
            return ReturnedProjectionSummary::Cycle;
        }

        if decl.body.is_empty() {
            let owning_trait = self
                .returned_view_traits_in_owner(owner_module)
                .into_iter()
                .find(|trait_info| {
                    trait_info.methods.get(&decl.name).is_some_and(|method| {
                        method.decl.span == decl.span && method.decl.receiver == decl.receiver
                    })
                });
            if let Some(trait_info) = owning_trait {
                let qualified_trait_name =
                    format!("{}.{}", trait_info.module_name, trait_info.decl.name);
                let mut projections = BTreeSet::new();
                let mut found = false;
                let mut deferred_cycle = false;
                for trait_impl in self.trait_impls_in_scope().filter(|trait_impl| {
                    trait_impl.trait_name == trait_info.decl.name
                        || trait_impl.trait_name == qualified_trait_name
                }) {
                    let Some(method) = trait_impl.methods.get(&decl.name) else {
                        continue;
                    };
                    found = true;
                    match self.returned_view_projection_summary(
                        &method.decl,
                        &trait_impl.module_name,
                        seen,
                        &HashMap::new(),
                    ) {
                        ReturnedProjectionSummary::Known(nested) => projections.extend(nested),
                        ReturnedProjectionSummary::Cycle => deferred_cycle = true,
                        ReturnedProjectionSummary::Unknown => {
                            seen.remove(&key);
                            return ReturnedProjectionSummary::Unknown;
                        }
                    }
                }
                seen.remove(&key);
                return if !projections.is_empty() {
                    ReturnedProjectionSummary::Known(projections)
                } else if found && deferred_cycle {
                    ReturnedProjectionSummary::Cycle
                } else {
                    ReturnedProjectionSummary::Unknown
                };
            }
        }

        let mut returns = Vec::new();
        collect_view_return_entries(&decl.body, &mut BTreeMap::new(), &mut returns);
        if returns.is_empty() {
            seen.remove(&key);
            return ReturnedProjectionSummary::Unknown;
        }

        let mut projections = BTreeSet::new();
        let mut deferred_cycle = false;
        for (value, aliases) in &returns {
            match self.returned_view_expr_projection_summary_with_aliases(
                owner_module,
                decl,
                value,
                &contract.origin,
                seen,
                aliases,
                &mut BTreeSet::new(),
                substitutions,
            ) {
                ReturnedProjectionSummary::Known(returned) => projections.extend(returned),
                ReturnedProjectionSummary::Cycle => {
                    deferred_cycle = true;
                }
                ReturnedProjectionSummary::Unknown => {
                    seen.remove(&key);
                    return ReturnedProjectionSummary::Unknown;
                }
            }
        }
        seen.remove(&key);
        if projections.is_empty() {
            if deferred_cycle {
                ReturnedProjectionSummary::Cycle
            } else {
                ReturnedProjectionSummary::Unknown
            }
        } else {
            ReturnedProjectionSummary::Known(projections)
        }
    }

    pub(super) fn returned_view_free_function_info_in_owner(
        &self,
        owner_module: &str,
        decl: &crate::ast::FunctionDecl,
    ) -> Option<&FunctionInfo> {
        let matches = |info: &&FunctionInfo| {
            info.module_name == owner_module
                && info.decl.name == decl.name
                && info.decl.span == decl.span
        };
        if owner_module == self.module_name {
            return self.functions.values().find(matches);
        }
        self.module_namespace(owner_module).and_then(|namespace| {
            namespace
                .all_functions
                .values()
                .chain(namespace.functions.values())
                .find(matches)
        })
    }

    pub(super) fn returned_view_classes_in_owner<'b>(
        &'b self,
        owner_module: &str,
    ) -> Vec<&'b ClassInfo> {
        if owner_module == self.module_name {
            self.classes
                .values()
                .filter(|class| class.module_name == owner_module)
                .collect::<Vec<_>>()
        } else {
            self.module_namespace(owner_module)
                .map(|namespace| {
                    namespace
                        .all_classes
                        .values()
                        .chain(namespace.classes.values())
                        .filter(|class| class.module_name == owner_module)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
    }

    pub(super) fn returned_view_traits_in_owner<'b>(
        &'b self,
        owner_module: &str,
    ) -> Vec<&'b TraitInfo> {
        if owner_module == self.module_name {
            self.traits
                .values()
                .filter(|trait_info| trait_info.module_name == owner_module)
                .collect::<Vec<_>>()
        } else {
            self.module_namespace(owner_module)
                .map(|namespace| {
                    namespace
                        .all_traits
                        .values()
                        .chain(namespace.traits.values())
                        .filter(|trait_info| trait_info.module_name == owner_module)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
    }

    pub(super) fn returned_view_summary_context(
        &self,
        owner_module: &str,
        decl: &crate::ast::FunctionDecl,
    ) -> ReturnedViewSummaryContext {
        let mut context = ReturnedViewSummaryContext::default();
        if let Some(info) = self.returned_view_free_function_info_in_owner(owner_module, decl) {
            for (param, ty) in decl.params.iter().zip(&info.signature.params) {
                context.locals.insert(param.name.clone(), ty.clone());
            }
            context.type_param_bounds = info.type_param_bounds.clone();
            return context;
        }
        for class in self.returned_view_classes_in_owner(owner_module) {
            let Some(method) = class.methods.get(&decl.name).filter(|method| {
                method.decl.span == decl.span && method.decl.receiver == decl.receiver
            }) else {
                continue;
            };
            let class_name = if class.module_name == self.module_name {
                class.decl.name.clone()
            } else {
                format!("{}.{}", class.module_name, class.decl.name)
            };
            context.locals.insert(
                "self".to_string(),
                Type::Named(
                    class_name,
                    class
                        .decl
                        .type_params
                        .iter()
                        .cloned()
                        .map(Type::TypeParam)
                        .collect(),
                ),
            );
            for (param, ty) in decl.params.iter().zip(&method.signature.params) {
                context.locals.insert(param.name.clone(), ty.clone());
            }
            context.type_param_bounds = class.type_param_bounds.clone();
            context
                .type_param_bounds
                .extend(method.type_param_bounds.clone());
            return context;
        }
        for trait_impl in self
            .trait_impls_in_scope()
            .filter(|trait_impl| trait_impl.module_name == owner_module)
        {
            let Some(method) = trait_impl.methods.get(&decl.name).filter(|method| {
                method.decl.span == decl.span && method.decl.receiver == decl.receiver
            }) else {
                continue;
            };
            context
                .locals
                .insert("self".to_string(), trait_impl.for_type.clone());
            for (param, ty) in decl.params.iter().zip(&method.signature.params) {
                context.locals.insert(param.name.clone(), ty.clone());
            }
            context.type_param_bounds = trait_impl.type_param_bounds.clone();
            context
                .type_param_bounds
                .extend(method.type_param_bounds.clone());
            return context;
        }
        for trait_info in self.returned_view_traits_in_owner(owner_module) {
            let Some(method) = trait_info.methods.get(&decl.name).filter(|method| {
                method.decl.span == decl.span && method.decl.receiver == decl.receiver
            }) else {
                continue;
            };
            context
                .locals
                .insert("self".to_string(), Type::TypeParam("Self".to_string()));
            for (param, ty) in decl.params.iter().zip(&method.signature.params) {
                context.locals.insert(param.name.clone(), ty.clone());
            }
            context.type_param_bounds = method.type_param_bounds.clone();
            return context;
        }
        context
    }

    pub(super) fn returned_view_signature_in_owner(
        &self,
        owner_module: &str,
        decl: &crate::ast::FunctionDecl,
    ) -> Option<FunctionSignature> {
        if let Some(info) = self.returned_view_free_function_info_in_owner(owner_module, decl) {
            return Some(info.signature.clone());
        }
        for class in self.returned_view_classes_in_owner(owner_module) {
            if let Some(method) = class.methods.get(&decl.name).filter(|method| {
                method.decl.span == decl.span && method.decl.receiver == decl.receiver
            }) {
                return Some(method.signature.clone());
            }
        }
        for trait_impl in self
            .trait_impls_in_scope()
            .filter(|trait_impl| trait_impl.module_name == owner_module)
        {
            if let Some(method) = trait_impl.methods.get(&decl.name).filter(|method| {
                method.decl.span == decl.span && method.decl.receiver == decl.receiver
            }) {
                return Some(method.signature.clone());
            }
        }
        for trait_info in self.returned_view_traits_in_owner(owner_module) {
            if let Some(method) = trait_info.methods.get(&decl.name).filter(|method| {
                method.decl.span == decl.span && method.decl.receiver == decl.receiver
            }) {
                return Some(method.signature.clone());
            }
        }
        None
    }

    pub(super) fn returned_view_class_in_owner(
        &self,
        owner_module: &str,
        expr: &Expr,
        context: &ReturnedViewSummaryContext,
    ) -> Option<&ClassInfo> {
        let expr = grouped_specialized_expr(expr);
        match &expr.kind {
            ExprKind::Name(name) if !context.locals.contains_key(name) => self
                .returned_view_classes_in_owner(owner_module)
                .into_iter()
                .find(|class| class.decl.name == *name),
            ExprKind::Member { object, field } => {
                let module_path = self.infer_module_path_in_owner(owner_module, object)?;
                let namespace = self.module_namespace(&module_path)?;
                namespace
                    .all_classes
                    .get(field)
                    .or_else(|| namespace.classes.get(field))
            }
            _ => None,
        }
    }

    pub(super) fn returned_view_trait_in_owner(
        &self,
        owner_module: &str,
        name: &str,
    ) -> Option<&TraitInfo> {
        self.returned_view_traits_in_owner(owner_module)
            .into_iter()
            .find(|trait_info| {
                trait_info.decl.name == name
                    || format!("{}.{}", trait_info.module_name, trait_info.decl.name) == name
            })
            .or_else(|| self.traits.get(name))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn returned_view_expr_type_in_owner(
        &self,
        owner_module: &str,
        enclosing_decl: &crate::ast::FunctionDecl,
        expr: &Expr,
        context: &ReturnedViewSummaryContext,
        substitutions: &HashMap<String, Type>,
        aliases: &BTreeMap<String, Expr>,
        alias_seen: &mut BTreeSet<String>,
    ) -> Option<Type> {
        let expr = grouped_specialized_expr(expr);
        match &expr.kind {
            ExprKind::Name(name) => {
                if let Some(ty) = context.locals.get(name) {
                    return Some(substitute_type(ty, substitutions));
                }
                let source = aliases.get(name)?;
                if !alias_seen.insert(name.clone()) {
                    return None;
                }
                let ty = self.returned_view_expr_type_in_owner(
                    owner_module,
                    enclosing_decl,
                    source,
                    context,
                    substitutions,
                    aliases,
                    alias_seen,
                );
                alias_seen.remove(name);
                ty
            }
            ExprKind::Member { object, field } => {
                let object_ty = self.returned_view_expr_type_in_owner(
                    owner_module,
                    enclosing_decl,
                    object,
                    context,
                    substitutions,
                    aliases,
                    alias_seen,
                )?;
                self.resolve_member_type(&object_ty, field, expr.span).ok()
            }
            ExprKind::Index { object, index } => {
                let Type::Tuple(elements) = self.returned_view_expr_type_in_owner(
                    owner_module,
                    enclosing_decl,
                    object,
                    context,
                    substitutions,
                    aliases,
                    alias_seen,
                )?
                else {
                    return None;
                };
                let ExprKind::Int(index) = index.kind else {
                    return None;
                };
                elements.get(usize::try_from(index).ok()?).cloned()
            }
            ExprKind::Call { callee, args } => {
                let (decl, owner) = self.returned_view_function_in_owner(
                    owner_module,
                    enclosing_decl,
                    callee,
                    substitutions,
                    aliases,
                )?;
                let call_substitutions = self.returned_view_nested_call_type_substitutions(
                    owner_module,
                    enclosing_decl,
                    &decl,
                    &owner,
                    callee,
                    args,
                    substitutions,
                    aliases,
                );
                self.returned_view_signature_in_owner(&owner, &decl)
                    .map(|signature| substitute_type(&signature.return_type, &call_substitutions))
            }
            _ => None,
        }
    }

    pub(super) fn returned_view_function_in_owner(
        &self,
        owner_module: &str,
        enclosing_decl: &crate::ast::FunctionDecl,
        callee: &Expr,
        substitutions: &HashMap<String, Type>,
        aliases: &BTreeMap<String, Expr>,
    ) -> Option<(crate::ast::FunctionDecl, String)> {
        let callee = grouped_specialized_expr(callee);
        match &callee.kind {
            ExprKind::Name(name) => {
                let info = if owner_module == self.module_name {
                    self.functions
                        .get(name)
                        .filter(|info| info.module_name == owner_module)
                } else {
                    self.module_registry
                        .get(owner_module)
                        .and_then(|namespace| {
                            namespace
                                .all_functions
                                .get(name)
                                .or_else(|| namespace.functions.get(name))
                                .filter(|info| info.module_name == owner_module)
                        })
                }?;
                Some((info.decl.clone(), info.module_name.clone()))
            }
            ExprKind::Member { object, field } => {
                if let Some(module_path) = self.infer_module_path_in_owner(owner_module, object) {
                    let namespace = self.module_namespace(&module_path)?;
                    let info = namespace
                        .functions
                        .get(field)
                        .or_else(|| namespace.all_functions.get(field))?;
                    return Some((info.decl.clone(), info.module_name.clone()));
                }
                let mut context = self.returned_view_summary_context(owner_module, enclosing_decl);
                let symbolic_receiver_ty = self.returned_view_expr_type_in_owner(
                    owner_module,
                    enclosing_decl,
                    object,
                    &context,
                    &HashMap::new(),
                    aliases,
                    &mut BTreeSet::new(),
                );
                let required_trait = match symbolic_receiver_ty {
                    Some(Type::TypeParam(type_param)) => {
                        let mut matches = context
                            .type_param_bounds
                            .get(&type_param)
                            .into_iter()
                            .flatten()
                            .filter_map(|bound| {
                                let trait_info = self.returned_view_trait_in_owner(
                                    owner_module,
                                    &bound.trait_name,
                                )?;
                                trait_info.methods.contains_key(field).then(|| {
                                    (
                                        bound.trait_name.clone(),
                                        trait_info.decl.name.clone(),
                                        format!(
                                            "{}.{}",
                                            trait_info.module_name, trait_info.decl.name
                                        ),
                                    )
                                })
                            });
                        let matched = matches.next();
                        matched.filter(|_| matches.next().is_none())
                    }
                    _ => None,
                };
                for ty in context.locals.values_mut() {
                    *ty = substitute_type(ty, substitutions);
                }
                if let Some(class) =
                    self.returned_view_class_in_owner(owner_module, object, &context)
                {
                    if let Some(method) = class
                        .methods
                        .get(field)
                        .filter(|method| method.decl.receiver.is_none())
                    {
                        return Some((method.decl.clone(), class.module_name.clone()));
                    }
                }
                let receiver_ty = self.returned_view_expr_type_in_owner(
                    owner_module,
                    enclosing_decl,
                    object,
                    &context,
                    substitutions,
                    aliases,
                    &mut BTreeSet::new(),
                )?;
                if let Type::Named(class_name, _) = &receiver_ty {
                    if let Some(class) = self.resolve_class_info(class_name) {
                        if let Some(method) = class.methods.get(field) {
                            return Some((method.decl.clone(), class.module_name.clone()));
                        }
                    }
                }
                if let Type::TypeParam(type_param) = &receiver_ty {
                    let mut matches = context
                        .type_param_bounds
                        .get(type_param)
                        .into_iter()
                        .flatten()
                        .filter_map(|bound| {
                            let trait_info =
                                self.returned_view_trait_in_owner(owner_module, &bound.trait_name)?;
                            let method = trait_info.methods.get(field)?;
                            Some((trait_info, method))
                        });
                    let (trait_info, method) = matches.next()?;
                    if matches.next().is_none() {
                        return Some((method.decl.clone(), trait_info.module_name.clone()));
                    }
                    return None;
                }
                if let Some((bound_name, trait_name, qualified_trait_name)) = required_trait {
                    let mut matches = self
                        .trait_impls_in_scope()
                        .filter(|trait_impl| {
                            trait_impl.trait_name == bound_name
                                || trait_impl.trait_name == trait_name
                                || trait_impl.trait_name == qualified_trait_name
                        })
                        .filter_map(|trait_impl| {
                            self.trait_impl_substitutions(trait_impl, &receiver_ty)?;
                            let method = trait_impl.methods.get(field)?;
                            Some((
                                crate::sema::trait_impl_specificity(trait_impl),
                                trait_impl,
                                method,
                            ))
                        })
                        .collect::<Vec<_>>();
                    matches.sort_by_key(|(specificity, _, _)| std::cmp::Reverse(*specificity));
                    let best_specificity =
                        matches.first().map(|(specificity, _, _)| *specificity)?;
                    let mut best = matches
                        .into_iter()
                        .take_while(|(specificity, _, _)| *specificity == best_specificity);
                    let (_, trait_impl, method) = best.next()?;
                    if best.next().is_some() {
                        return None;
                    }
                    return Some((method.decl.clone(), trait_impl.module_name.clone()));
                }
                self.trait_impls_in_scope()
                    .filter_map(|trait_impl| {
                        let substitutions =
                            self.trait_impl_substitutions(trait_impl, &receiver_ty)?;
                        let method = trait_impl.methods.get(field)?;
                        Some((
                            crate::sema::trait_impl_specificity(trait_impl),
                            trait_impl,
                            method,
                            substitutions,
                        ))
                    })
                    .max_by_key(|(specificity, _, _, _)| *specificity)
                    .map(|(_, trait_impl, method, _)| {
                        (method.decl.clone(), trait_impl.module_name.clone())
                    })
            }
            _ => None,
        }
    }

    pub(super) fn infer_module_path_in_owner(
        &self,
        owner_module: &str,
        expr: &Expr,
    ) -> Option<String> {
        match &grouped_specialized_expr(expr).kind {
            ExprKind::Name(name) => {
                let imported = if owner_module == self.module_name {
                    self.current_module_namespace()
                        .map(|namespace| &namespace.imported_modules)
                        .unwrap_or(self.imported_modules)
                } else {
                    &self.module_registry.get(owner_module)?.imported_modules
                };
                imported.get(name).map(|namespace| namespace.path.clone())
            }
            ExprKind::Member { object, field } => {
                let parent = self.infer_module_path_in_owner(owner_module, object)?;
                self.module_namespace(&parent)
                    .and_then(|namespace| namespace.modules.get(field))
                    .map(|namespace| namespace.path.clone())
            }
            _ => None,
        }
    }

    pub(super) fn returned_view_call_parent(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Option<String> {
        let expr = grouped_expr(expr);
        if let ExprKind::Member { object, .. } | ExprKind::Index { object, .. } = &expr.kind {
            return self.returned_view_call_parent(object, locals);
        }
        let ExprKind::Call { callee, args } = &expr.kind else {
            return None;
        };
        let (decl, receiver, _owner_module) = self.returned_view_callee(callee, locals).ok()??;
        let contract = decl.view_return.as_ref()?;
        let origin = if contract.origin == "self" {
            receiver.as_ref()?
        } else {
            let origin_index = decl
                .params
                .iter()
                .position(|param| param.name == contract.origin)?;
            let ordered = bind_call_arguments(
                &format!("callable `{}`", decl.name),
                &callable_params_from_decl(&decl.params),
                args,
                callee.span,
                CallConvention::PositionalOrNamed,
            )
            .ok()?;
            &ordered.get(origin_index).copied().flatten()?.value
        };
        let name = self
            .borrow_call_place(origin)
            .map(|place| place.root)
            .or_else(|| self.returned_view_call_parent(origin, locals))?;
        locals
            .get(&name)
            .is_some_and(|binding| binding.view.is_some())
            .then_some(name)
    }

    pub(super) fn returned_view_call_kind(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<crate::ast::ViewKind>> {
        let expr = grouped_expr(expr);
        if let ExprKind::Member { object, .. } | ExprKind::Index { object, .. } = &expr.kind {
            return self.returned_view_call_kind(object, locals);
        }
        let ExprKind::Call { callee, .. } = &expr.kind else {
            return Ok(None);
        };
        // Capability lookup is metadata-only and is often performed after the
        // ordinary call checker has consumed an owned receiver.  Resolve it
        // from the receiver's static type without replaying move-state errors;
        // the call and any returned place are validated on their original
        // source-order pass.
        let mut contract_locals = locals.clone();
        for binding in contract_locals.values_mut() {
            binding.moved = false;
            binding.moved_fields.clear();
            binding.stale_match_borrow_place = None;
        }
        let contract = self
            .returned_view_callee(callee, &mut contract_locals)?
            .and_then(|(decl, _, _)| decl.view_return);
        Ok(contract.map(|contract| {
            if contract.mutable {
                crate::ast::ViewKind::Mutable
            } else {
                crate::ast::ViewKind::Shared
            }
        }))
    }

    pub(super) fn direct_view_value_kind(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<crate::ast::ViewKind>> {
        match &grouped_expr(expr).kind {
            ExprKind::Name(name) => Ok(locals
                .get(name)
                .and_then(|binding| binding.view.as_ref())
                .map(|view| view.kind)),
            ExprKind::Call { .. } => self.returned_view_call_kind(expr, locals),
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                let kind = self.direct_view_value_kind(object, locals)?;
                let Some(kind) = kind else {
                    return Ok(None);
                };
                // Reading a Copy projection materializes a fresh pointee
                // value; it does not move or store the surrounding view
                // descriptor. Non-Copy projections remain borrowed values
                // and must stay within view-aware contexts.
                let mut type_locals = locals.clone();
                let projected_ty = self.type_of_expr(expr, &mut type_locals)?;
                if self.is_copy_type(&projected_ty) {
                    Ok(None)
                } else {
                    Ok(Some(kind))
                }
            }
            _ => Ok(None),
        }
    }

    pub(super) fn reject_view_value_for_passing(
        &self,
        expr: &Expr,
        passing: ReceiverKind,
        destination: &str,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.reject_mutable_returned_view_value(expr, locals, passing == ReceiverKind::BorrowMut)?;
        if passing == ReceiverKind::Value {
            self.reject_owned_view_value(expr, locals, destination)?;
        }
        Ok(())
    }

    pub(super) fn reject_mutable_returned_view_value(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        allow_root: bool,
    ) -> Result<()> {
        let expr = grouped_expr(expr);
        if matches!(expr.kind, ExprKind::Call { .. }) {
            if self.returned_view_call_kind(expr, locals)? == Some(crate::ast::ViewKind::Mutable)
                && !allow_root
            {
                return Err(Diagnostic::coded_at(
                    "AU3010",
                    expr.span,
                    "a mutable returned view requires a mutable view binding or immediate mutable reborrow",
                )
                .with_help(
                    "bind it with `view mut`, or pass the call directly to a `mut` parameter",
                ));
            }
            let ExprKind::Call { callee, args } = &expr.kind else {
                unreachable!();
            };
            let mut metadata_locals = locals.clone();
            for binding in metadata_locals.values_mut() {
                binding.moved = false;
                binding.moved_fields.clear();
                binding.stale_match_borrow_place = None;
            }
            if let Some((decl, receiver, _)) =
                self.returned_view_callee(callee, &mut metadata_locals)?
            {
                if let Some(receiver) = receiver.as_ref() {
                    self.reject_view_value_for_passing(
                        receiver,
                        decl.receiver.unwrap_or(ReceiverKind::Value),
                        "an owned method receiver",
                        locals,
                    )?;
                }
                let ordered = bind_call_arguments(
                    &format!("callable `{}`", decl.name),
                    &callable_params_from_decl(&decl.params),
                    args,
                    callee.span,
                    CallConvention::PositionalOrNamed,
                )?;
                for (argument, passing) in ordered.into_iter().zip(
                    decl.params
                        .iter()
                        .map(|param| resolve_param_passing(param.mode)),
                ) {
                    if let Some(argument) = argument {
                        self.reject_mutable_returned_view_value(
                            &argument.value,
                            locals,
                            passing == ReceiverKind::BorrowMut,
                        )?;
                    }
                }
                return Ok(());
            }
            if let ExprKind::Member { object, field } = &grouped_specialized_expr(callee).kind {
                if let Ok(Type::Named(receiver_name, _)) =
                    self.type_of_member_object_expr(object, &mut metadata_locals)
                {
                    if let Some(member) = BuiltinMember::resolve(&receiver_name, field) {
                        self.reject_view_value_for_passing(
                            object,
                            member.receiver_passing(),
                            "an owned builtin receiver",
                            locals,
                        )?;
                        // TaskGroup's fixed builtin signature stops at the
                        // target callable. Every later argument is forwarded
                        // to that callable and may therefore use its target
                        // parameter name. Do not bind those names against the
                        // builtin itself. Direct view roots deliberately reach
                        // the ordinary TaskGroup checker so it can emit the
                        // task-boundary-specific AU3008; nested mutable views
                        // still receive their normal context validation.
                        if matches!(
                            member,
                            BuiltinMember::TaskGroupStart
                                | BuiltinMember::TaskGroupStartSoon
                                | BuiltinMember::TaskGroupStartWithStack
                                | BuiltinMember::TaskGroupStartSoonWithStack
                        ) {
                            for argument in args {
                                self.reject_mutable_returned_view_value(
                                    &argument.value,
                                    locals,
                                    true,
                                )?;
                            }
                            return Ok(());
                        }
                        let ordered = member.bind_args(args, callee.span)?;
                        for (index, argument) in ordered.into_iter().enumerate() {
                            if let (Some(argument), Some(passing)) =
                                (argument, member.argument_passing(index))
                            {
                                self.reject_view_value_for_passing(
                                    &argument.value,
                                    passing,
                                    "an owned builtin argument",
                                    locals,
                                )?;
                            }
                        }
                        return Ok(());
                    }
                }
            }
            let callable_params = match self.type_of_expr(callee, &mut metadata_locals) {
                Ok(Type::Function { params, .. }) => Some(params),
                Ok(Type::Closure { params, .. }) => Some(*params),
                _ => None,
            };
            if let Some(params) = callable_params {
                let bindings = params
                    .iter()
                    .map(|param| crate::call::CallableParam {
                        name: &param.name,
                        required: !param.has_default,
                    })
                    .collect::<Vec<_>>();
                let ordered = bind_call_arguments(
                    "function value",
                    &bindings,
                    args,
                    callee.span,
                    if params.iter().any(|param| param.name.is_empty()) {
                        CallConvention::PositionalOnly
                    } else {
                        CallConvention::PositionalOrNamed
                    },
                )?;
                for (argument, param) in ordered.into_iter().zip(params) {
                    if let Some(argument) = argument {
                        self.reject_mutable_returned_view_value(
                            &argument.value,
                            locals,
                            param.passing == ReceiverKind::BorrowMut,
                        )?;
                    }
                }
                return Ok(());
            }
            // Builtins and structural callees that do not expose a resolved
            // mutable parameter here must treat nested returned views as
            // ordinary value expressions. Their ordinary call checker still
            // performs the full type and ownership validation.
            for argument in args {
                self.reject_mutable_returned_view_value(&argument.value, locals, false)?;
            }
            return Ok(());
        }
        match &expr.kind {
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.reject_mutable_returned_view_value(element, locals, false)?;
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.reject_mutable_returned_view_value(&entry.key, locals, false)?;
                    self.reject_mutable_returned_view_value(&entry.value, locals, false)?;
                }
            }
            ExprKind::IsNone { value: expr, .. }
            | ExprKind::Unary { expr, .. }
            | ExprKind::Cast { expr, .. }
            | ExprKind::Specialize { expr, .. }
            | ExprKind::Try(expr) => {
                self.reject_mutable_returned_view_value(expr, locals, false)?;
            }
            ExprKind::Member { object, .. } => {
                self.reject_mutable_returned_view_value(object, locals, allow_root)?;
            }
            ExprKind::Binary { left, right, .. }
            | ExprKind::Membership {
                value: left,
                container: right,
                ..
            } => {
                self.reject_mutable_returned_view_value(left, locals, false)?;
                self.reject_mutable_returned_view_value(right, locals, false)?;
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.reject_mutable_returned_view_value(condition, locals, false)?;
                self.reject_mutable_returned_view_value(then_expr, locals, false)?;
                self.reject_mutable_returned_view_value(else_expr, locals, false)?;
            }
            ExprKind::Index { object, index } => {
                self.reject_mutable_returned_view_value(object, locals, allow_root)?;
                self.reject_mutable_returned_view_value(index, locals, false)?;
            }
            ExprKind::Slice {
                object, start, end, ..
            } => {
                self.reject_mutable_returned_view_value(object, locals, false)?;
                if let Some(start) = start {
                    self.reject_mutable_returned_view_value(start, locals, false)?;
                }
                if let Some(end) = end {
                    self.reject_mutable_returned_view_value(end, locals, false)?;
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.reject_mutable_returned_view_value(scrutinee, locals, false)?;
                for arm in arms {
                    self.reject_mutable_returned_view_value(&arm.value, locals, false)?;
                }
            }
            ExprKind::CompareChain { first, links } => {
                self.reject_mutable_returned_view_value(first, locals, false)?;
                for link in links {
                    self.reject_mutable_returned_view_value(&link.operand, locals, false)?;
                }
            }
            ExprKind::Group(_) | ExprKind::Call { .. } => unreachable!("normalized above"),
            ExprKind::FString(parts) => {
                for part in parts {
                    match part {
                        crate::ast::FormatPart::Literal(_) => {}
                        crate::ast::FormatPart::Expr(value)
                        | crate::ast::FormatPart::Formatted { expr: value, .. } => {
                            self.reject_mutable_returned_view_value(value, locals, false)?;
                        }
                    }
                }
            }
            ExprKind::Name(_)
            | ExprKind::Int(_)
            | ExprKind::DurationNanos(_)
            | ExprKind::BuiltinOmitted
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_)
            | ExprKind::Comprehension { .. }
            | ExprKind::Lambda { .. } => {}
        }
        Ok(())
    }

    pub(super) fn reject_owned_view_value(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        destination: &str,
    ) -> Result<()> {
        if self.direct_view_value_kind(expr, locals)?.is_none() {
            return Ok(());
        }
        Err(Diagnostic::coded_at(
            "AU3010",
            expr.span,
            format!("a view cannot be stored as {destination}"),
        )
        .with_help("keep it in a matching `view` binding, or produce an explicit owned value"))
    }

    pub(super) fn expire_views_before(
        &self,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) {
        for binding in locals.values_mut() {
            if binding
                .view
                .as_ref()
                .is_some_and(|view| span_precedes(view.last_use, span))
            {
                binding.view = None;
            }
            binding
                .closure_loans
                .retain(|loan| !span_precedes(loan.last_use, span));
        }
    }

    pub(super) fn expire_views_unused_in_branch(
        &self,
        body: &[Stmt],
        control_last_uses: &BTreeMap<String, crate::diag::Span>,
        locals: &mut HashMap<String, LocalBinding>,
    ) {
        for (name, binding) in locals.iter_mut() {
            if block_references_name(body, name) {
                continue;
            }
            let control_last_use = control_last_uses.get(name).copied();
            if binding
                .view
                .as_ref()
                .is_some_and(|view| control_last_use == Some(view.last_use))
            {
                binding.view = None;
            }
            binding
                .closure_loans
                .retain(|loan| control_last_use != Some(loan.last_use));
        }
    }

    pub(super) fn view_descends_from(
        &self,
        descendant: &str,
        ancestor: &str,
        locals: &HashMap<String, LocalBinding>,
    ) -> bool {
        if locals.get(descendant).is_some_and(|binding| {
            binding
                .view
                .iter()
                .chain(&binding.closure_loans)
                .any(|view| {
                    view.parent.as_deref() == Some(ancestor) || view.ancestors.contains(ancestor)
                })
        }) {
            return true;
        }
        let mut pending = vec![descendant.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(binding) = locals.get(&name) else {
                continue;
            };
            for parent in binding
                .view
                .iter()
                .chain(&binding.closure_loans)
                .filter_map(|view| view.parent.as_deref())
            {
                if parent == ancestor {
                    return true;
                }
                pending.push(parent.to_string());
            }
        }
        false
    }

    pub(super) fn view_ancestor_names(
        &self,
        parent: Option<&str>,
        locals: &HashMap<String, LocalBinding>,
    ) -> BTreeSet<String> {
        let mut ancestors = BTreeSet::new();
        let Some(parent) = parent else {
            return ancestors;
        };
        ancestors.insert(parent.to_string());
        if let Some(binding) = locals.get(parent) {
            for view in binding.view.iter().chain(&binding.closure_loans) {
                if let Some(parent) = &view.parent {
                    ancestors.insert(parent.clone());
                }
                ancestors.extend(view.ancestors.iter().cloned());
            }
        }
        ancestors
    }

    pub(super) fn access_uses_view_or_descendant(
        &self,
        active_view: &str,
        through_view: Option<&str>,
        locals: &HashMap<String, LocalBinding>,
    ) -> bool {
        through_view.is_some_and(|through| {
            (through == active_view && !self.view_has_active_descendant(active_view, locals))
                || self.view_descends_from(through, active_view, locals)
        })
    }

    pub(super) fn view_has_active_descendant(
        &self,
        ancestor: &str,
        locals: &HashMap<String, LocalBinding>,
    ) -> bool {
        locals.keys().any(|candidate| {
            candidate != ancestor && self.view_descends_from(candidate, ancestor, locals)
        })
    }

    pub(super) fn current_view_return(&self) -> Option<&crate::ast::ViewReturn> {
        match &self.closure_owner {
            ClosureOwner::Function(name) => self
                .functions
                .get(name)
                .and_then(|function| function.decl.view_return.as_ref()),
            ClosureOwner::ClassMethod {
                class_name,
                method_name,
            } => self
                .classes
                .get(class_name)
                .and_then(|class| class.methods.get(method_name))
                .and_then(|method| method.decl.view_return.as_ref()),
            ClosureOwner::TraitMethod {
                trait_name,
                method_name,
            } => self
                .traits
                .get(trait_name)
                .and_then(|trait_info| trait_info.methods.get(method_name))
                .and_then(|method| method.decl.view_return.as_ref()),
            ClosureOwner::TraitImplMethod {
                trait_name,
                for_type,
                method_name,
            } => self
                .trait_impls
                .iter()
                .find(|trait_impl| {
                    trait_impl.trait_name == *trait_name
                        && trait_impl.for_type.to_string() == *for_type
                })
                .and_then(|trait_impl| trait_impl.methods.get(method_name))
                .and_then(|method| method.decl.view_return.as_ref()),
            ClosureOwner::TopLevel => None,
        }
    }

    pub(super) fn ensure_view_loan_available(
        &self,
        requested: &PlacePath,
        kind: crate::ast::ViewKind,
        parent: Option<&str>,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if kind == crate::ast::ViewKind::Mutable {
            self.ensure_place_not_frozen(requested, span, locals)?;
        }
        for (name, binding) in locals {
            for active in binding.view.iter().chain(&binding.closure_loans) {
                if binding.view.is_some()
                    && parent.is_some_and(|parent| {
                        parent == name || self.view_descends_from(parent, name, locals)
                    })
                {
                    continue;
                }
                if !active.source.overlaps(requested) {
                    continue;
                }
                if kind == crate::ast::ViewKind::Shared
                    && active.kind == crate::ast::ViewKind::Shared
                {
                    continue;
                }
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    span,
                    format!(
                        "cannot create {} view of `{requested}` while {} loan held by `{name}` remains live",
                        if kind == crate::ast::ViewKind::Mutable { "mutable" } else { "shared" },
                        if active.kind == crate::ast::ViewKind::Mutable { "mutable" } else { "shared" },
                    ),
                )
                .with_secondary(active.created_at, format!("loan held by `{name}` starts here"))
                .with_secondary(active.last_use, format!("last use of `{name}` keeps this loan live"))
                .with_help("remove the later use, shorten its scope, or borrow a proven-disjoint field"));
            }
        }
        Ok(())
    }

    pub(super) fn ensure_place_not_locked_by_view(
        &self,
        place: &PlacePath,
        through_view: Option<&str>,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let place = self.canonicalize_view_place(place.clone(), locals);
        for (name, binding) in locals {
            for view in binding.view.iter().chain(&binding.closure_loans) {
                if view.source.overlaps(&place)
                    && !self.access_uses_view_or_descendant(name, through_view, locals)
                {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        span,
                        format!(
                            "cannot mutate `{place}` while {} view `{name}` remains live",
                            if view.kind == crate::ast::ViewKind::Mutable {
                                "mutable"
                            } else {
                                "shared"
                            }
                        ),
                    )
                    .with_secondary(view.created_at, format!("view `{name}` starts here"))
                    .with_secondary(
                        view.last_use,
                        format!("last use of view `{name}` keeps this loan live"),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn ensure_place_readable(
        &self,
        place: &PlacePath,
        through_view: Option<&str>,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let place = self.canonicalize_view_place(place.clone(), locals);
        for (name, binding) in locals {
            for view in binding.view.iter().chain(&binding.closure_loans) {
                if view.kind != crate::ast::ViewKind::Mutable
                    || !view.source.overlaps(&place)
                    || self.access_uses_view_or_descendant(name, through_view, locals)
                {
                    continue;
                }
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    span,
                    format!(
                        "cannot read `{place}` while mutable view `{name}` remains live; use the view instead"
                    ),
                )
                .with_secondary(view.created_at, format!("mutable view `{name}` starts here"))
                .with_secondary(
                    view.last_use,
                    format!("last use of view `{name}` keeps this loan live"),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn expr_borrow_info(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<BorrowSourceInfo>> {
        match &expr.kind {
            ExprKind::Name(name) => Ok(locals.get(name).and_then(|binding| {
                (binding.passing != ReceiverKind::Value).then(|| BorrowSourceInfo {
                    origin: binding
                        .borrow_origin
                        .clone()
                        .unwrap_or_else(|| name.clone()),
                    passing: binding.passing,
                    match_borrow_place: binding.match_borrow_place.clone(),
                    shared_match_scrutinee: binding.shared_match_scrutinee.clone(),
                })
            })),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => self.expr_borrow_info(inner, locals),
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                if self.is_payload_free_variant_expr(expr) {
                    return Ok(None);
                }
                if let Some((module_path, function_name)) = self.qualified_module_item(expr) {
                    if self
                        .module_namespace(&module_path)
                        .is_some_and(|namespace| {
                            namespace.functions.contains_key(&function_name)
                                || namespace.all_functions.contains_key(&function_name)
                        })
                    {
                        // Module-qualified function values are immediate Copy
                        // pointers. Avoid retyping a contextual generic value
                        // without the expected function type merely to prove
                        // that it cannot carry a borrow.
                        return Ok(None);
                    }
                }
                let value_ty = self.type_of_expr(expr, locals)?;
                if self.is_copy_type(&value_ty) {
                    Ok(None)
                } else {
                    self.expr_borrow_info(object, locals)
                }
            }
            // A call, match, or conditional now always produces an owned
            // value: ADR-0022 removed borrowed returns, so no loan can
            // propagate out through one of these.
            _ => Ok(None),
        }
    }

    pub(super) fn collect_expr_borrowed_places(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        self.collect_expr_call_places(expr, locals, places, false)
    }

    pub(super) fn collect_expr_consumed_places(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        let mut call_places = Vec::new();
        self.collect_expr_call_places(expr, locals, &mut call_places, true)?;
        places.extend(
            call_places
                .into_iter()
                .filter(|place| place.passing == ReceiverKind::Value),
        );
        Ok(())
    }

    pub(super) fn collect_result_place_accesses(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        passing: ReceiverKind,
        label: &str,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Name(_) => {
                let Some(path) = self.borrow_call_place(expr) else {
                    return Ok(());
                };
                let Some(ty) = self.place_path_type(&path, locals, expr.span)? else {
                    return Ok(());
                };
                if Self::result_place_access_is_retained(passing, self.is_copy_type(&ty)) {
                    places.push(BorrowedCallPlace {
                        path,
                        passing,
                        param_name: label.to_string(),
                        origin_span: expr.span,
                    });
                }
                Ok(())
            }
            ExprKind::Member { object, field } => self.collect_projected_member_result_accesses(
                object,
                ProjectedField {
                    name: field,
                    span: expr.span,
                },
                locals,
                passing,
                label,
                places,
            ),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. }
            | ExprKind::Try(inner) => {
                self.collect_result_place_accesses(inner, locals, passing, label, places)
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.collect_result_place_accesses(element, locals, passing, label, places)?;
                }
                Ok(())
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.collect_result_place_accesses(&entry.key, locals, passing, label, places)?;
                    self.collect_result_place_accesses(
                        &entry.value,
                        locals,
                        passing,
                        label,
                        places,
                    )?;
                }
                Ok(())
            }
            ExprKind::Comprehension { output, clauses } => {
                for clause in clauses {
                    self.collect_result_place_accesses(
                        &clause.iterable,
                        locals,
                        passing,
                        label,
                        places,
                    )?;
                    for filter in &clause.filters {
                        self.collect_result_place_accesses(filter, locals, passing, label, places)?;
                    }
                }
                match output {
                    ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                        self.collect_result_place_accesses(value, locals, passing, label, places)
                    }
                    ComprehensionOutput::Map { key, value } => {
                        self.collect_result_place_accesses(key, locals, passing, label, places)?;
                        self.collect_result_place_accesses(value, locals, passing, label, places)
                    }
                }
            }
            ExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.collect_result_place_accesses(then_expr, locals, passing, label, places)?;
                self.collect_result_place_accesses(else_expr, locals, passing, label, places)
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    self.collect_result_place_accesses(&arm.value, locals, passing, label, places)?;
                }
                Ok(())
            }
            ExprKind::Call { .. } => {
                if passing == ReceiverKind::Value {
                    return Ok(());
                }
                let mut place_locals = locals.clone();
                if let Some(path) = self.view_place(expr, &mut place_locals)? {
                    places.push(BorrowedCallPlace {
                        path,
                        passing,
                        param_name: label.to_string(),
                        origin_span: expr.span,
                    });
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// A copy-typed argument passed by value leaves no retained access, but a
    /// copy-typed place passed as `borrow` or `borrow mut` still aliases the
    /// place for the rest of the call.
    pub(super) fn result_place_access_is_retained(passing: ReceiverKind, is_copy: bool) -> bool {
        !(is_copy && passing == ReceiverKind::Value)
    }

    pub(super) fn collect_projected_member_result_accesses(
        &self,
        object: &Expr,
        field: ProjectedField<'_>,
        locals: &HashMap<String, LocalBinding>,
        passing: ReceiverKind,
        label: &str,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        match &object.kind {
            ExprKind::Group(inner) => self.collect_projected_member_result_accesses(
                inner, field, locals, passing, label, places,
            ),
            ExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.collect_projected_member_result_accesses(
                    then_expr, field, locals, passing, label, places,
                )?;
                self.collect_projected_member_result_accesses(
                    else_expr, field, locals, passing, label, places,
                )
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    self.collect_projected_member_result_accesses(
                        &arm.value, field, locals, passing, label, places,
                    )?;
                }
                Ok(())
            }
            _ => {
                let member_expr = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(object.clone()),
                        field: field.name.to_string(),
                    },
                    span: field.span,
                };
                let Some(path) = self.borrow_call_place(&member_expr) else {
                    return Ok(());
                };
                let Some(ty) = self.place_path_type(&path, locals, field.span)? else {
                    return Ok(());
                };
                if Self::result_place_access_is_retained(passing, self.is_copy_type(&ty)) {
                    places.push(BorrowedCallPlace {
                        path,
                        passing,
                        param_name: label.to_string(),
                        origin_span: field.span,
                    });
                }
                Ok(())
            }
        }
    }

    pub(super) fn collect_expr_call_places(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
        include_consumed: bool,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. }
            | ExprKind::Try(inner) => {
                self.collect_expr_call_places(inner, locals, places, include_consumed)
            }
            ExprKind::Unary { expr: inner, .. } | ExprKind::IsNone { value: inner, .. } => {
                self.collect_expr_call_places(inner, locals, places, include_consumed)
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_expr_call_places(left, locals, places, include_consumed)?;
                self.collect_expr_call_places(right, locals, places, include_consumed)
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.collect_expr_call_places(condition, locals, places, include_consumed)?;
                self.collect_expr_call_places(then_expr, locals, places, include_consumed)?;
                self.collect_expr_call_places(else_expr, locals, places, include_consumed)
            }
            ExprKind::Call { callee, args } => {
                self.collect_expr_call_places(callee, locals, places, include_consumed)?;
                for argument in args {
                    self.collect_expr_call_places(
                        &argument.value,
                        locals,
                        places,
                        include_consumed,
                    )?;
                }
                self.collect_call_borrowed_places(callee, args, locals, places, include_consumed)
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.collect_expr_call_places(element, locals, places, include_consumed)?;
                }
                Ok(())
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.collect_expr_call_places(&entry.key, locals, places, include_consumed)?;
                    self.collect_expr_call_places(&entry.value, locals, places, include_consumed)?;
                }
                Ok(())
            }
            ExprKind::Comprehension { output, clauses } => {
                for clause in clauses {
                    self.collect_expr_call_places(
                        &clause.iterable,
                        locals,
                        places,
                        include_consumed,
                    )?;
                    for filter in &clause.filters {
                        self.collect_expr_call_places(filter, locals, places, include_consumed)?;
                    }
                }
                match output {
                    ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                        self.collect_expr_call_places(value, locals, places, include_consumed)
                    }
                    ComprehensionOutput::Map { key, value } => {
                        self.collect_expr_call_places(key, locals, places, include_consumed)?;
                        self.collect_expr_call_places(value, locals, places, include_consumed)
                    }
                }
            }
            ExprKind::FString(parts) => {
                for part in parts {
                    match part {
                        crate::ast::FormatPart::Expr(value)
                        | crate::ast::FormatPart::Formatted { expr: value, .. } => {
                            self.collect_expr_call_places(value, locals, places, include_consumed)?;
                        }
                        crate::ast::FormatPart::Literal(_) => {}
                    }
                }
                Ok(())
            }
            ExprKind::Member { object, .. } => {
                self.collect_expr_call_places(object, locals, places, include_consumed)
            }
            ExprKind::Index { object, index } => {
                self.collect_expr_call_places(object, locals, places, include_consumed)?;
                self.collect_expr_call_places(index, locals, places, include_consumed)
            }
            ExprKind::Slice {
                object, start, end, ..
            } => {
                self.collect_expr_call_places(object, locals, places, include_consumed)?;
                if let Some(start) = start {
                    self.collect_expr_call_places(start, locals, places, include_consumed)?;
                }
                if let Some(end) = end {
                    self.collect_expr_call_places(end, locals, places, include_consumed)?;
                }
                Ok(())
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.collect_expr_call_places(scrutinee, locals, places, include_consumed)?;
                for arm in arms {
                    self.collect_expr_call_places(&arm.value, locals, places, include_consumed)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn collect_expr_place_reads(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        label: &str,
        places: &mut Vec<BorrowedCallPlace>,
    ) {
        let mut push_place = |path: PlacePath, span: crate::diag::Span| {
            if locals.contains_key(&path.root) {
                places.push(BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Borrow,
                    param_name: label.to_string(),
                    origin_span: span,
                });
            }
        };
        match &expr.kind {
            ExprKind::Name(name) => push_place(PlacePath::root(name.clone()), expr.span),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. }
            | ExprKind::Try(inner)
            | ExprKind::Unary { expr: inner, .. }
            | ExprKind::IsNone { value: inner, .. } => {
                self.collect_expr_place_reads(inner, locals, label, places)
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_expr_place_reads(left, locals, label, places);
                self.collect_expr_place_reads(right, locals, label, places);
            }
            ExprKind::Membership {
                value, container, ..
            } => {
                self.collect_expr_place_reads(value, locals, label, places);
                self.collect_expr_place_reads(container, locals, label, places);
            }
            ExprKind::CompareChain { first, links } => {
                self.collect_expr_place_reads(first, locals, label, places);
                for link in links {
                    self.collect_expr_place_reads(&link.operand, locals, label, places);
                }
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.collect_expr_place_reads(condition, locals, label, places);
                self.collect_expr_place_reads(then_expr, locals, label, places);
                self.collect_expr_place_reads(else_expr, locals, label, places);
            }
            ExprKind::Call { callee, args } => {
                self.collect_expr_place_reads(callee, locals, label, places);
                for argument in args {
                    self.collect_expr_place_reads(&argument.value, locals, label, places);
                }
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.collect_expr_place_reads(element, locals, label, places);
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.collect_expr_place_reads(&entry.key, locals, label, places);
                    self.collect_expr_place_reads(&entry.value, locals, label, places);
                }
            }
            ExprKind::Comprehension { output, clauses } => {
                for clause in clauses {
                    self.collect_expr_place_reads(&clause.iterable, locals, label, places);
                    for filter in &clause.filters {
                        self.collect_expr_place_reads(filter, locals, label, places);
                    }
                }
                match output {
                    ComprehensionOutput::List(value) | ComprehensionOutput::Set(value) => {
                        self.collect_expr_place_reads(value, locals, label, places);
                    }
                    ComprehensionOutput::Map { key, value } => {
                        self.collect_expr_place_reads(key, locals, label, places);
                        self.collect_expr_place_reads(value, locals, label, places);
                    }
                }
            }
            ExprKind::FString(parts) => {
                for part in parts {
                    match part {
                        crate::ast::FormatPart::Expr(value)
                        | crate::ast::FormatPart::Formatted { expr: value, .. } => {
                            self.collect_expr_place_reads(value, locals, label, places);
                        }
                        crate::ast::FormatPart::Literal(_) => {}
                    }
                }
            }
            ExprKind::Member { object, .. } => {
                if let Some(path) = self.borrow_call_place(expr) {
                    push_place(path, expr.span);
                } else {
                    self.collect_expr_place_reads(object, locals, label, places);
                }
            }
            ExprKind::Index { object, index } => {
                if let Some(path) = self.borrow_call_place(object) {
                    push_place(path, object.span);
                } else {
                    self.collect_expr_place_reads(object, locals, label, places);
                }
                self.collect_expr_place_reads(index, locals, label, places);
            }
            ExprKind::Slice {
                object, start, end, ..
            } => {
                self.collect_expr_place_reads(object, locals, label, places);
                if let Some(start) = start {
                    self.collect_expr_place_reads(start, locals, label, places);
                }
                if let Some(end) = end {
                    self.collect_expr_place_reads(end, locals, label, places);
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.collect_expr_place_reads(scrutinee, locals, label, places);
                for arm in arms {
                    self.collect_expr_place_reads(&arm.value, locals, label, places);
                }
            }
            ExprKind::Lambda { params, body, .. } => {
                let bound = params
                    .iter()
                    .map(|param| param.name.clone())
                    .collect::<BTreeSet<_>>();
                let mut seen = BTreeSet::new();
                let mut captures = Vec::new();
                Self::collect_lambda_capture_uses(body, &bound, &mut seen, &mut captures);
                for (name, span) in captures {
                    push_place(PlacePath::root(name), span);
                }
            }
            ExprKind::Int(_)
            | ExprKind::DurationNanos(_)
            | ExprKind::BuiltinOmitted
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_) => {}
        }
    }

    pub(super) fn collect_call_borrowed_places(
        &self,
        callee: &Expr,
        args: &[Argument],
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
        include_consumed: bool,
    ) -> Result<()> {
        let mut locals_for_resolution = locals.clone();
        let (base_callee, _) = self.peel_specialization(callee);
        match &base_callee.kind {
            ExprKind::Name(name) => {
                let Some(function) = self.resolve_function_info(name) else {
                    return Ok(());
                };
                let ordered_args = bind_call_arguments(
                    &format!("function `{}`", function.decl.name),
                    &callable_params_from_decl(&function.decl.params),
                    args,
                    callee.span,
                    CallConvention::PositionalOrNamed,
                )?;
                for ((argument, param), passing) in ordered_args
                    .into_iter()
                    .zip(function.decl.params.iter())
                    .zip(function.signature.param_passings.iter().copied())
                {
                    let Some(argument) = argument else {
                        continue;
                    };
                    if let Some(path) = self.borrow_call_place(&argument.value) {
                        if passing == ReceiverKind::Value
                            && (!include_consumed
                                || self
                                    .place_path_type(&path, locals, argument.value.span)?
                                    .is_none_or(|ty| self.is_copy_type(&ty)))
                        {
                            continue;
                        }
                        places.push(BorrowedCallPlace {
                            path,
                            passing,
                            param_name: param.name.clone(),
                            origin_span: argument.value.span,
                        });
                    }
                }
                Ok(())
            }
            ExprKind::Member { object, field } => {
                let (base_object, _) = self.peel_specialization(object);
                if let ExprKind::Name(type_name) = &base_object.kind {
                    if !locals.contains_key(type_name) {
                        if let Some(associated) =
                            BuiltinAssociatedFunction::resolve(type_name, field)
                        {
                            let ordered_args = associated.bind_args(args, callee.span)?;
                            for (index, argument) in ordered_args.into_iter().enumerate() {
                                let Some(argument) = argument else {
                                    continue;
                                };
                                let Some(passing) = associated.argument_passing(index) else {
                                    continue;
                                };
                                if let Some(path) = self.borrow_call_place(&argument.value) {
                                    if passing == ReceiverKind::Value
                                        && (!include_consumed
                                            || self
                                                .place_path_type(
                                                    &path,
                                                    locals,
                                                    argument.value.span,
                                                )?
                                                .is_none_or(|ty| self.is_copy_type(&ty)))
                                    {
                                        continue;
                                    }
                                    places.push(BorrowedCallPlace {
                                        path,
                                        passing,
                                        param_name: associated
                                            .argument_name(index)
                                            .unwrap_or("argument")
                                            .to_string(),
                                        origin_span: argument.value.span,
                                    });
                                }
                            }
                            return Ok(());
                        }
                    }
                }
                if self.is_enum_constructor_object(object) {
                    return Ok(());
                }
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                self.collect_method_borrowed_places(
                                    object,
                                    (callee, args),
                                    method,
                                    locals,
                                    places,
                                    include_consumed,
                                )?;
                                return Ok(());
                            }
                        }
                    }
                }
                let receiver_ty = self.type_of_expr(object, &mut locals_for_resolution)?;
                let Type::Named(receiver_name, _) = receiver_ty else {
                    return Ok(());
                };
                let Some(class_info) = self.resolve_class_info(&receiver_name) else {
                    return Ok(());
                };
                let Some(method) = class_info.methods.get(field) else {
                    return Ok(());
                };
                self.collect_method_borrowed_places(
                    object,
                    (callee, args),
                    method,
                    locals,
                    places,
                    include_consumed,
                )
            }
            _ => Ok(()),
        }
    }

    pub(super) fn collect_method_borrowed_places(
        &self,
        object: &Expr,
        call: (&Expr, &[Argument]),
        method: &MethodInfo,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
        include_consumed: bool,
    ) -> Result<()> {
        let (callee, args) = call;
        if let Some(receiver_passing) = method.decl.receiver {
            if let Some(path) = self.borrow_call_place(object) {
                if receiver_passing != ReceiverKind::Value
                    || (include_consumed
                        && self
                            .place_path_type(&path, locals, object.span)?
                            .is_some_and(|ty| !self.is_copy_type(&ty)))
                {
                    places.push(BorrowedCallPlace {
                        path,
                        passing: receiver_passing,
                        param_name: "self".to_string(),
                        origin_span: object.span,
                    });
                }
            }
        }
        let ordered_args = bind_call_arguments(
            "method call",
            &callable_params_from_decl(&method.decl.params),
            args,
            callee.span,
            CallConvention::PositionalOrNamed,
        )?;
        for ((argument, param), passing) in ordered_args
            .into_iter()
            .zip(method.decl.params.iter())
            .zip(method.signature.param_passings.iter().copied())
        {
            let Some(argument) = argument else {
                continue;
            };
            if let Some(path) = self.borrow_call_place(&argument.value) {
                if passing == ReceiverKind::Value
                    && (!include_consumed
                        || self
                            .place_path_type(&path, locals, argument.value.span)?
                            .is_none_or(|ty| self.is_copy_type(&ty)))
                {
                    continue;
                }
                places.push(BorrowedCallPlace {
                    path,
                    passing,
                    param_name: param.name.clone(),
                    origin_span: argument.value.span,
                });
            }
        }
        Ok(())
    }

    pub(super) fn retained_place_access(
        &self,
        expr: &Expr,
        ty: &Type,
        passing: ReceiverKind,
        label: &str,
    ) -> Option<BorrowedCallPlace> {
        if self.is_copy_type(ty) {
            return None;
        }
        self.borrow_call_place(expr).map(|path| BorrowedCallPlace {
            path,
            passing,
            param_name: label.to_string(),
            origin_span: expr.span,
        })
    }

    pub(super) fn retained_call_place_access(
        &self,
        expr: &Expr,
        ty: &Type,
        passing: ReceiverKind,
        label: &str,
    ) -> Option<BorrowedCallPlace> {
        if passing == ReceiverKind::Value && self.is_copy_type(ty) {
            return None;
        }
        self.borrow_call_place(expr).map(|path| BorrowedCallPlace {
            path,
            passing,
            param_name: label.to_string(),
            origin_span: expr.span,
        })
    }

    pub(super) fn retained_path_access(
        &self,
        path: PlacePath,
        ty: &Type,
        passing: ReceiverKind,
        label: &str,
        origin_span: crate::diag::Span,
    ) -> Option<BorrowedCallPlace> {
        if passing == ReceiverKind::Value && self.is_copy_type(ty) {
            return None;
        }
        Some(BorrowedCallPlace {
            path,
            passing,
            param_name: label.to_string(),
            origin_span,
        })
    }

    pub(super) fn reject_builtin_receiver_argument_overlap(
        &self,
        builtin_member: BuiltinMember,
        object: &Expr,
        receiver_ty: &Type,
        args: &[Argument],
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let retained_receiver = self
            .retained_place_access(
                object,
                receiver_ty,
                builtin_member.receiver_passing(),
                "method receiver",
            )
            .into_iter()
            .collect::<Vec<_>>();
        if retained_receiver.is_empty() {
            return Ok(());
        }
        let mut argument_accesses = Vec::new();
        for argument in args {
            self.collect_expr_borrowed_places(&argument.value, locals, &mut argument_accesses)?;
            self.collect_expr_consumed_places(&argument.value, locals, &mut argument_accesses)?;
        }
        if builtin_member.variadic_argument_passing().is_none() {
            let ordered_args = builtin_member.bind_args(args, object.span)?;
            for (index, argument) in ordered_args.into_iter().enumerate() {
                let Some(argument) = argument else {
                    continue;
                };
                if builtin_member.argument_passing(index) != Some(ReceiverKind::Value) {
                    continue;
                }
                let Some(path) = self.borrow_call_place(&argument.value) else {
                    continue;
                };
                if !locals.contains_key(&path.root) {
                    continue;
                }
                let argument_ty =
                    self.type_of_expr_without_move_state(&argument.value, locals, None)?;
                if self.is_copy_type(&argument_ty) {
                    continue;
                }
                argument_accesses.push(BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Value,
                    param_name: "owned method argument".to_string(),
                    origin_span: argument.value.span,
                });
            }
        } else if builtin_member.variadic_argument_passing() == Some(ReceiverKind::Value) {
            for argument in args.iter().skip(1) {
                let Some(path) = self.borrow_call_place(&argument.value) else {
                    continue;
                };
                if !locals.contains_key(&path.root) {
                    continue;
                }
                let argument_ty =
                    self.type_of_expr_without_move_state(&argument.value, locals, None)?;
                if self.is_copy_type(&argument_ty) {
                    continue;
                }
                argument_accesses.push(BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Value,
                    param_name: "owned variadic method argument".to_string(),
                    origin_span: argument.value.span,
                });
            }
        }
        for argument in args {
            self.collect_expr_place_reads(
                &argument.value,
                locals,
                "method argument read",
                &mut argument_accesses,
            );
        }
        self.reject_retained_access_overlap(&retained_receiver, &argument_accesses)
    }

    pub(super) fn reject_conditional_value_argument_overlap(
        &self,
        consumed: &Expr,
        consumed_ty: &Type,
        consumed_label: &str,
        args: &[Argument],
        skipped_argument: Option<&Argument>,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let retained = self
            .retained_place_access(consumed, consumed_ty, ReceiverKind::Value, consumed_label)
            .into_iter()
            .collect::<Vec<_>>();
        if retained.is_empty() {
            return Ok(());
        }
        let mut later_accesses = Vec::new();
        for argument in args {
            if skipped_argument.is_some_and(|skipped| std::ptr::eq(skipped, argument)) {
                continue;
            }
            self.collect_expr_borrowed_places(&argument.value, locals, &mut later_accesses)?;
            self.collect_expr_consumed_places(&argument.value, locals, &mut later_accesses)?;
            self.collect_expr_place_reads(
                &argument.value,
                locals,
                "argument read",
                &mut later_accesses,
            );
        }
        self.reject_retained_access_overlap(&retained, &later_accesses)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reject_retained_expr_overlap(
        &self,
        retained: &[BorrowedCallPlace],
        expr: &Expr,
        expr_ty: &Type,
        direct_passing: Option<ReceiverKind>,
        locals_before: &HashMap<String, LocalBinding>,
        locals_after: &HashMap<String, LocalBinding>,
        value_label: &str,
    ) -> Result<()> {
        if retained.is_empty() {
            return Ok(());
        }
        let mut later_accesses = Vec::new();
        self.collect_expr_borrowed_places(expr, locals_before, &mut later_accesses)?;
        if let Some(passing) = direct_passing {
            if let Some(access) =
                self.retained_call_place_access(expr, expr_ty, passing, value_label)
            {
                later_accesses.push(access);
            }
        }
        later_accesses.extend(self.newly_moved_place_accesses(
            locals_before,
            locals_after,
            value_label,
            expr.span,
        ));
        self.collect_expr_place_reads(expr, locals_before, value_label, &mut later_accesses);
        self.reject_retained_access_overlap(retained, &later_accesses)
    }

    pub(super) fn reject_retained_access_overlap(
        &self,
        retained: &[BorrowedCallPlace],
        later: &[BorrowedCallPlace],
    ) -> Result<()> {
        for current in later {
            for prior in retained {
                if !prior.path.overlaps(&current.path)
                    || (prior.passing == ReceiverKind::Borrow
                        && current.passing == ReceiverKind::Borrow)
                {
                    continue;
                }
                let action = match current.passing {
                    ReceiverKind::Borrow => "borrow",
                    ReceiverKind::BorrowMut => "mutably borrow",
                    ReceiverKind::Value => "consume",
                };
                // The recovery clause names the conflicting access. A pure
                // read or consumption has no mutation to sequence, so the
                // blanket "perform the mutation" wording misdescribed those
                // sites.
                let conflicting_access = match current.passing {
                    ReceiverKind::Borrow => "read",
                    ReceiverKind::BorrowMut => "mutation",
                    ReceiverKind::Value => "consumption",
                };
                let retained_state = match prior.passing {
                    ReceiverKind::Borrow => "shared-borrowed",
                    ReceiverKind::BorrowMut => "mutably borrowed",
                    ReceiverKind::Value => "reserved for consumption",
                };
                let origin_label = match prior.passing {
                    ReceiverKind::Borrow => {
                        format!("shared access for the {} begins here", prior.param_name)
                    }
                    ReceiverKind::BorrowMut => {
                        format!("mutable borrow for the {} begins here", prior.param_name)
                    }
                    ReceiverKind::Value => {
                        format!("consumption by the {} begins here", prior.param_name)
                    }
                };
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    current.origin_span,
                    format!(
                        "cannot {} `{}` while `{}` remains {} by the {}",
                        action, current.path, prior.path, retained_state, prior.param_name
                    ),
                )
                .with_secondary(prior.origin_span, origin_label)
                .with_help(format!(
                    "call `.clone()` before the expression when an independent value is intended, or perform the {conflicting_access} in a separate statement first"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn reject_expr_borrow_move_overlap(
        &self,
        borrowed_places: &[BorrowedCallPlace],
        moved_places: &[PlacePath],
        span: crate::diag::Span,
    ) -> Result<()> {
        for moved in moved_places {
            for borrowed in borrowed_places {
                if borrowed.path.overlaps(moved) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "cannot mix a move of `{}` with a borrow of `{}` in the same expression",
                            moved, borrowed.path
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn prepare_method_receiver_borrows(
        &self,
        method_name: &str,
        receiver_kind: Option<ReceiverKind>,
        object: &Expr,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Vec<BorrowedCallPlace>> {
        let Some(receiver_kind) = receiver_kind else {
            return Ok(Vec::new());
        };

        if receiver_kind == ReceiverKind::BorrowMut {
            self.require_mutable_receiver(object, method_name, span, locals)?;
        }

        if receiver_kind == ReceiverKind::Value {
            if let Some(place) = self.borrow_call_place(object) {
                self.ensure_place_not_shared_by_match_for_move(&place, object.span, locals)?;
            }
            self.consume_value_expr(object, locals)?;
            return Ok(Vec::new());
        }

        let mut borrowed_places = Vec::new();
        if let Some(place) = self.borrow_call_place(object) {
            borrowed_places.push(BorrowedCallPlace {
                path: place,
                passing: receiver_kind,
                param_name: "self".to_string(),
                origin_span: object.span,
            });
        }
        Ok(borrowed_places)
    }

    pub(super) fn reject_overlapping_borrow(
        &self,
        borrowed_places: &[BorrowedCallPlace],
        current_path: &PlacePath,
        current_passing: ReceiverKind,
        current_param_name: &str,
        callee_name: &str,
        span: crate::diag::Span,
    ) -> Result<()> {
        for prior in borrowed_places {
            if !prior.path.overlaps(current_path) {
                continue;
            }
            let shared =
                prior.passing == ReceiverKind::Borrow && current_passing == ReceiverKind::Borrow;
            if shared {
                continue;
            }
            let detail = match (current_passing, prior.passing) {
                (ReceiverKind::Value, ReceiverKind::Borrow) => format!(
                    "argument for parameter `{}` in {} overlaps borrow for parameter `{}`; consumed values must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Value, ReceiverKind::BorrowMut) => format!(
                    "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`; consumed values must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Value, ReceiverKind::Value)
                | (_, ReceiverKind::Value) => format!(
                    "argument for parameter `{}` in {} overlaps consumed argument for parameter `{}`; consumed values must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::BorrowMut, ReceiverKind::Borrow) => format!(
                    "argument for parameter `{}` in {} overlaps borrow for parameter `{}`; mutable borrows must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::BorrowMut, ReceiverKind::BorrowMut) => format!(
                    "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Borrow, ReceiverKind::BorrowMut) => format!(
                    "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`; mutable borrows must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Borrow, ReceiverKind::Borrow) => {
                    unreachable!("overlapping shared borrows are accepted above")
                }
            };
            let origin_label = match prior.passing {
                ReceiverKind::Borrow => {
                    format!(
                        "shared access for parameter `{}` begins here",
                        prior.param_name
                    )
                }
                ReceiverKind::BorrowMut => {
                    format!(
                        "mutable borrow for parameter `{}` begins here",
                        prior.param_name
                    )
                }
                ReceiverKind::Value => {
                    format!(
                        "value for parameter `{}` is consumed here",
                        prior.param_name
                    )
                }
            };
            let help = if current_passing == ReceiverKind::Value
                || prior.passing == ReceiverKind::Value
            {
                "pass non-overlapping places, or call `.clone()` before consuming a value that must remain available through shared access"
            } else {
                "pass non-overlapping places; shared accesses may overlap, but mutable access must remain exclusive"
            };
            return Err(Diagnostic::at(span, detail)
                .with_secondary(prior.origin_span, origin_label)
                .with_help(help));
        }
        Ok(())
    }

    pub(super) fn find_frozen_place_conflict(
        &self,
        place: &PlacePath,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<(PlacePath, crate::diag::Span)> {
        let place = self.canonicalize_view_place(place.clone(), locals);
        let binding = locals.get(&place.root)?;
        binding
            .frozen_places
            .iter()
            .find(|(frozen, _)| frozen.overlaps(&place))
            .map(|(frozen, origin)| (frozen.clone(), *origin))
    }

    pub(super) fn find_shared_match_place_conflict(
        &self,
        place: &PlacePath,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<(PlacePath, crate::diag::Span)> {
        let place = self.canonicalize_view_place(place.clone(), locals);
        let binding = locals.get(&place.root)?;
        binding
            .shared_match_places
            .iter()
            .find(|(shared, _)| shared.overlaps(&place))
            .map(|(shared, origin)| (shared.clone(), *origin))
    }

    pub(super) fn ensure_place_not_frozen(
        &self,
        place: &PlacePath,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.ensure_place_not_frozen_with_match_access(place, span, locals, false)
    }

    pub(super) fn ensure_place_mutation_allowed(
        &self,
        place: &PlacePath,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.ensure_place_not_frozen_with_match_access(place, span, locals, true)
    }

    pub(super) fn ensure_place_not_frozen_with_match_access(
        &self,
        access: &PlacePath,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
        allow_matching_access: bool,
    ) -> Result<()> {
        let place = self.canonicalize_view_place(access.clone(), locals);
        if let Some(active) = self
            .active_match_borrow_places
            .borrow()
            .iter()
            .find(|active| {
                active.physical.overlaps(&place)
                    && !(allow_matching_access
                        && active.access.root == access.root
                        && access
                            .projections
                            .is_descendant_of_or_equal(&active.access.projections))
            })
            .cloned()
        {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "cannot mutate `{place}` while `match mut` holds mutable access to `{}`",
                    active.physical
                ),
            )
            .with_help("mutate through the selected mutable pattern binding, or wait until the match arm ends"));
        }
        if let Some((shared, origin)) = self.find_shared_match_place_conflict(&place, locals) {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "cannot mutate `{place}` while `{shared}` remains shared by a bare match"
                ),
            )
            .with_secondary(origin, format!("bare `match {shared}` starts here"))
            .with_help(format!(
                "finish the selected match arm before mutating `{shared}`, or use `match mut {shared}` when the arm must update it"
            )));
        }
        if let Some((frozen, origin)) = self.find_frozen_place_conflict(&place, locals) {
            return Err(
                Diagnostic::at(
                    span,
                    format!(
                        "cannot mutate `{}` while `{}` is borrowed for iteration",
                        place, frozen
                    ),
                )
                .with_secondary(
                    origin,
                    format!("`{}` is borrowed for this loop here", frozen),
                )
                .with_help(format!(
                    "perform owner mutation after the loop; use `for item in mut {}:` when mutating elements through the loop binding",
                    frozen
                )),
            );
        }
        Ok(())
    }

    pub(super) fn ensure_place_not_frozen_for_move(
        &self,
        place: &PlacePath,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.ensure_place_not_shared_by_match_for_move(place, span, locals)?;
        if let Some((frozen, origin)) = self.find_frozen_place_conflict(place, locals) {
            return Err(
                Diagnostic::at(
                    span,
                    format!(
                        "cannot move `{}` while `{}` is borrowed for iteration",
                        place, frozen
                    ),
                )
                .with_secondary(
                    origin,
                    format!("`{}` is borrowed for this loop here", frozen),
                )
                .with_help(format!(
                    "finish iterating before moving `{}`, or iterate an owned clone when the owner must be consumed independently",
                    place.root
                )),
            );
        }
        Ok(())
    }

    pub(super) fn ensure_place_not_shared_by_match_for_move(
        &self,
        place: &PlacePath,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if let Some((shared, origin)) = self.find_shared_match_place_conflict(place, locals) {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "cannot consume `{place}` while `{shared}` remains shared by a bare match"
                ),
            )
            .with_secondary(origin, format!("bare `match {shared}` starts here"))
            .with_help(format!(
                "finish the selected match arm before consuming `{shared}`, or use `match own {shared}` when the match should consume it"
            )));
        }
        Ok(())
    }

    pub(super) fn begin_match_borrow_mut(
        &self,
        scrutinee: &Expr,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<PlacePath>> {
        let Some(access) = self.borrow_call_place(scrutinee) else {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                "`match mut` requires a mutable place scrutinee",
            ));
        };
        let place = self.canonicalize_view_place(access.clone(), locals);
        self.ensure_place_not_frozen(&place, span, locals)?;
        if !self.is_mutable_place(scrutinee, locals)? {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                "`match mut` requires a mutable place scrutinee",
            ));
        }
        if let Some(active) = self
            .active_match_borrow_places
            .borrow()
            .iter()
            .find(|active| active.physical.overlaps(&place))
            .cloned()
        {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "cannot start `match mut` on `{}` while an enclosing match already has mutable access to `{}`",
                    place, active.physical
                ),
            ));
        }
        self.active_match_borrow_places
            .borrow_mut()
            .push(ActiveMatchBorrow {
                physical: place.clone(),
                access,
            });
        Ok(Some(place))
    }

    pub(super) fn end_match_borrow_mut(&self, active_place: Option<PlacePath>) {
        if active_place.is_none() {
            return;
        }
        self.active_match_borrow_places.borrow_mut().pop();
    }
}
