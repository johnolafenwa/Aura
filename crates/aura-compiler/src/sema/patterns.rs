//! Pattern checking, match capabilities, guards, and exhaustiveness.

use super::*;

fn pattern_has_root_type_arm(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Type(_) => true,
        Pattern::Or(pattern) => pattern.alternatives.iter().any(pattern_has_root_type_arm),
        _ => false,
    }
}

fn pattern_contains_type_arm(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Type(_) => true,
        Pattern::Or(pattern) => pattern.alternatives.iter().any(pattern_contains_type_arm),
        Pattern::Variant(pattern) => pattern.subpatterns.iter().any(pattern_contains_type_arm),
        Pattern::Tuple(pattern) => pattern.elements.iter().any(pattern_contains_type_arm),
        _ => false,
    }
}

fn type_pattern_coverage_diagnostic(mut diagnostic: Diagnostic, has_type_arm: bool) -> Diagnostic {
    if has_type_arm
        && diagnostic.code == "AU2999"
        && [
            "non-exhaustive",
            "unreachable match arm",
            "duplicate match arm",
            "duplicate or subsumed alternative",
        ]
        .iter()
        .any(|prefix| diagnostic.message.starts_with(prefix))
    {
        diagnostic.code = "AU2013".to_string();
    }
    diagnostic
}

impl FunctionChecker<'_> {
    fn type_pattern_member(
        &self,
        pattern: &crate::ast::TypePattern,
        expected: &Type,
    ) -> Result<Type> {
        let member = lower_type(
            &pattern.ty,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &self.type_params,
        )?;
        if matches!(member, Type::Union(_)) {
            return Err(Diagnostic::coded_at(
                "AU2013",
                pattern.span,
                format!(
                    "type arm `{member}` denotes several union members; select one direct member"
                ),
            ));
        }
        if matches!(member, Type::TypeParam(_)) {
            return Err(Diagnostic::coded_at(
                "AU2013",
                pattern.span,
                format!("type arm `{member}` is not proved to denote one direct, disjoint member"),
            ));
        }
        let direct = match expected {
            Type::Union(union) => union.members.contains(&member),
            _ => &member == expected,
        };
        if !direct {
            return Err(Diagnostic::coded_at(
                "AU2013",
                pattern.span,
                format!("type arm `{member}` is not a direct member of `{expected}`"),
            ));
        }
        Ok(member)
    }

    fn is_none_type_pattern(pattern: &VariantPattern) -> bool {
        pattern.enum_name.is_none()
            && pattern.variant_name == "None"
            && pattern.subpatterns.is_empty()
    }

    fn typed_pattern_members(&self, pattern: &Pattern, expected: &Type) -> BTreeSet<String> {
        let key = |ty: &Type| ty.canonical_key(self.module_name, self.canonical_type_names);
        match pattern {
            Pattern::Or(pattern) => pattern
                .alternatives
                .iter()
                .flat_map(|alternative| self.typed_pattern_members(alternative, expected))
                .collect(),
            Pattern::Wildcard(_) | Pattern::Binding(_) => match expected {
                Type::Union(union) => union.members.iter().map(key).collect(),
                _ => BTreeSet::from([key(expected)]),
            },
            Pattern::Type(pattern) => self
                .type_pattern_member(pattern, expected)
                .map(|member| BTreeSet::from([key(&member)]))
                .unwrap_or_default(),
            Pattern::Variant(pattern) if Self::is_none_type_pattern(pattern) => {
                let contains_none = match expected {
                    Type::Union(union) => union.members.contains(&Type::Unit),
                    _ => expected == &Type::Unit,
                };
                if contains_none {
                    BTreeSet::from([key(&Type::Unit)])
                } else {
                    BTreeSet::new()
                }
            }
            _ => BTreeSet::new(),
        }
    }

    fn typed_patterns_missing(&self, patterns: &[&Pattern], expected: &Type) -> Vec<String> {
        let covered = patterns
            .iter()
            .flat_map(|pattern| self.typed_pattern_members(pattern, expected))
            .collect::<BTreeSet<_>>();
        let members = match expected {
            Type::Union(union) => union.members.as_slice(),
            _ => std::slice::from_ref(expected),
        };
        members
            .iter()
            .filter(|member| {
                !covered
                    .contains(&member.canonical_key(self.module_name, self.canonical_type_names))
            })
            .map(ToString::to_string)
            .collect()
    }

    pub(super) fn check_match(
        &self,
        match_stmt: &MatchStmt,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        let control_last_uses = locals
            .keys()
            .filter_map(|name| {
                last_name_reference_span_in_match(match_stmt, name).map(|span| (name.clone(), span))
            })
            .collect::<BTreeMap<_, _>>();
        let shared_scrutinee =
            self.shared_match_scrutinee_name(&match_stmt.scrutinee, match_stmt.capability);
        let shared_match_place = (match_stmt.capability == ReceiverKind::Borrow)
            .then(|| self.shared_match_place(&match_stmt.scrutinee, locals))
            .flatten();
        let active_match_borrow = if match_stmt.capability == ReceiverKind::BorrowMut {
            self.begin_match_borrow_mut(&match_stmt.scrutinee, match_stmt.span, locals)?
        } else {
            None
        };
        let result = (|| {
            let scrutinee_ty = self.type_of_expr(&match_stmt.scrutinee, locals)?;
            let scrutinee_ty =
                self.narrowed_scrutinee_type(&match_stmt.scrutinee, scrutinee_ty, locals);
            if match_stmt.capability == ReceiverKind::Value {
                if self.is_copy_type(&scrutinee_ty) {
                    if let Some(place) = self.borrow_call_place(&match_stmt.scrutinee) {
                        self.ensure_place_not_shared_by_match_for_move(
                            &place,
                            match_stmt.scrutinee.span,
                            locals,
                        )?;
                    }
                } else {
                    self.consume_match_scrutinee_expr(&match_stmt.scrutinee, locals)?;
                }
            }

            if match_stmt.arms.is_empty() {
                return Err(Diagnostic::at(
                    match_stmt.span,
                    "`match` requires at least one `case` arm",
                ));
            }

            if matches!(scrutinee_ty, Type::Union(_) | Type::Unit)
                || match_stmt
                    .arms
                    .iter()
                    .any(|arm| pattern_has_root_type_arm(&arm.pattern))
            {
                let mut prior = Vec::new();
                let mut arm_states = Vec::new();
                let mut all_return = true;
                for arm in &match_stmt.arms {
                    let mut arm_locals = locals.clone();
                    if let Some(place) = shared_match_place.as_ref() {
                        self.retain_shared_match_place(place, match_stmt.span, &mut arm_locals);
                    }
                    self.bind_pattern_locals(
                        &arm.pattern,
                        &scrutinee_ty,
                        &mut arm_locals,
                        match_stmt.capability,
                        active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                        shared_scrutinee.as_deref(),
                    )?;
                    if self.patterns_cover_pattern(&prior, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU2013",
                            self.pattern_span(&arm.pattern),
                            "duplicate or unreachable type arm",
                        ));
                    }
                    self.check_match_guard(
                        arm.guard.as_ref(),
                        &arm.pattern,
                        match_stmt.capability,
                        &mut arm_locals,
                    )?;
                    self.expire_views_unused_in_branch(
                        &arm.body,
                        &control_last_uses,
                        &mut arm_locals,
                    );
                    let flow = self.check_block(
                        &arm.body,
                        &mut arm_locals,
                        return_type,
                        loop_depth,
                        allow_return,
                    )?;
                    if flow == BlockFlow::FallsThrough {
                        all_return = false;
                        arm_states.push(arm_locals);
                    }
                    if arm.guard.is_none() {
                        prior.push(&arm.pattern);
                    }
                }
                let missing = self.missing_patterns_for_type(&prior, &scrutinee_ty);
                if !missing.is_empty() {
                    return Err(Diagnostic::coded_at(
                        "AU2013",
                        match_stmt.span,
                        format!("match does not cover {}", missing.join(", ")),
                    ));
                }
                self.merge_control_flow_moves(locals, &arm_states.iter().collect::<Vec<_>>());
                return Ok(if all_return {
                    BlockFlow::AlwaysReturns
                } else {
                    BlockFlow::FallsThrough
                });
            }

            if let Some(variants) = self.enum_variants_for_type(&scrutinee_ty) {
                let Type::Named(enum_name, _type_args) = &scrutinee_ty else {
                    unreachable!("enum scrutinee types should be named");
                };
                let scrutinee_enum_name = self.canonical_enum_name(enum_name);
                let mut covered = BTreeMap::<String, crate::diag::Span>::new();
                let mut patterns_by_variant =
                    BTreeMap::<String, Vec<crate::ast::VariantPattern>>::new();
                let mut wildcard_span = None;
                let mut all_return = true;
                let mut arm_states = Vec::new();

                for (index, arm) in match_stmt.arms.iter().enumerate() {
                    let mut arm_locals = locals.clone();
                    if self.is_copy_type(&scrutinee_ty) || pattern_contains_type_arm(&arm.pattern) {
                        if let Some(place) = shared_match_place.as_ref() {
                            self.retain_shared_match_place(place, match_stmt.span, &mut arm_locals);
                        }
                    }
                    match &arm.pattern {
                        Pattern::Type(_) => {
                            unreachable!("root type arms use the typed match checker")
                        }
                        Pattern::Or(pattern) => {
                            self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                match_stmt.capability,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                        Pattern::Wildcard(span) => {
                            if arm.guard.is_none() {
                                if index + 1 != match_stmt.arms.len() {
                                    return Err(Diagnostic::at(
                                        *span,
                                        "wildcard match arm must be the final `case`",
                                    ));
                                }
                                wildcard_span = Some(*span);
                            }
                        }
                        Pattern::Literal(pattern) => {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                "match over `{}` expects enum variant patterns, not literal `{}`",
                                enum_name,
                                self.render_literal_pattern(pattern)
                            ),
                            ));
                        }
                        Pattern::Binding(binding) => {
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                match_stmt.capability,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                            if arm.guard.is_none() {
                                if index + 1 != match_stmt.arms.len() {
                                    return Err(Diagnostic::at(
                                        binding.span,
                                        "catch-all match arm must be the final `case`",
                                    ));
                                }
                                wildcard_span = Some(binding.span);
                            }
                        }
                        Pattern::Tuple(tuple) => {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "match over `{}` expects enum variant patterns, not a tuple pattern",
                                    enum_name
                                ),
                            ));
                        }
                        Pattern::Variant(pattern) => {
                            let pattern_enum_name =
                                if let Some(pattern_enum_name) = &pattern.enum_name {
                                    if pattern_enum_name == enum_name {
                                        pattern_enum_name.clone()
                                    } else if let Some(pattern_enum_info) =
                                        self.resolve_enum_info(pattern_enum_name)
                                    {
                                        self.canonical_enum_info_name(
                                            pattern_enum_name,
                                            pattern_enum_info,
                                        )
                                    } else {
                                        return Err(Diagnostic::at(
                                            pattern.span,
                                            format!(
                                                "unknown enum `{}` in match pattern",
                                                pattern_enum_name
                                            ),
                                        ));
                                    }
                                } else {
                                    scrutinee_enum_name.clone()
                                };
                            if pattern_enum_name != scrutinee_enum_name {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "match arm expects enum `{}`, found pattern for `{}`",
                                        scrutinee_enum_name, pattern_enum_name
                                    ),
                                ));
                            }

                            let Some(variant_payload) = variants
                                .iter()
                                .find(|(name, _)| name == &pattern.variant_name)
                                .map(|(_, payload)| payload.clone())
                            else {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "enum `{}` has no variant `{}`",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            };

                            let covers_entire_variant = arm.guard.is_none()
                                && self.variant_pattern_covers_payloads(pattern, &variant_payload);
                            if covers_entire_variant {
                                if let Some(previous) =
                                    covered.insert(pattern.variant_name.clone(), pattern.span)
                                {
                                    return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "duplicate match arm for `{}.{}` (previously matched at {})",
                                        scrutinee_enum_name, pattern.variant_name, previous
                                    ),
                                ));
                                }
                            }
                            if arm.guard.is_none() {
                                patterns_by_variant
                                    .entry(pattern.variant_name.clone())
                                    .or_default()
                                    .push(pattern.clone());
                            }

                            if pattern.subpatterns.is_empty() && !variant_payload.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` carries a payload and must bind it",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if variant_payload.is_empty() && !pattern.subpatterns.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` does not carry a payload",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if pattern.subpatterns.len() != variant_payload.len() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` expects {} pattern payload{}, found {}",
                                        scrutinee_enum_name,
                                        pattern.variant_name,
                                        variant_payload.len(),
                                        if variant_payload.len() == 1 { "" } else { "s" },
                                        pattern.subpatterns.len()
                                    ),
                                ));
                            }
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                match_stmt.capability,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                    }

                    let prior_patterns = match_stmt.arms[..index]
                        .iter()
                        .filter(|previous_arm| previous_arm.guard.is_none())
                        .map(|previous_arm| &previous_arm.pattern)
                        .collect::<Vec<_>>();
                    if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::at(
                            self.pattern_span(&arm.pattern),
                            "unreachable match arm",
                        ));
                    }

                    self.check_match_guard(
                        arm.guard.as_ref(),
                        &arm.pattern,
                        match_stmt.capability,
                        &mut arm_locals,
                    )?;
                    self.expire_views_unused_in_branch(
                        &arm.body,
                        &control_last_uses,
                        &mut arm_locals,
                    );
                    let arm_flow = self.check_block(
                        &arm.body,
                        &mut arm_locals,
                        return_type,
                        loop_depth,
                        allow_return,
                    )?;
                    if arm_flow == BlockFlow::FallsThrough {
                        all_return = false;
                        arm_states.push(arm_locals);
                    }
                }

                for (variant_name, payloads) in &variants {
                    if covered.contains_key(variant_name) {
                        continue;
                    }
                    let Some(patterns) = patterns_by_variant.get(variant_name) else {
                        continue;
                    };
                    let pattern_refs = patterns.iter().collect::<Vec<_>>();
                    if self.variant_patterns_cover_payloads_union(&pattern_refs, payloads) {
                        let span = patterns
                            .first()
                            .map(|pattern| pattern.span)
                            .unwrap_or(match_stmt.span);
                        covered.insert(variant_name.clone(), span);
                    }
                }

                let branch_states = arm_states.iter().collect::<Vec<_>>();
                self.merge_control_flow_moves(locals, &branch_states);

                let pattern_refs = match_stmt
                    .arms
                    .iter()
                    .filter(|arm| arm.guard.is_none())
                    .map(|arm| &arm.pattern)
                    .collect::<Vec<_>>();
                let missing = self.missing_patterns_for_type(&pattern_refs, &scrutinee_ty);
                if wildcard_span.is_none() && !missing.is_empty() {
                    let rendered = missing
                        .iter()
                        .map(|name| format!("`{}`", name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(Diagnostic::at(
                        match_stmt.span,
                        format!(
                            "non-exhaustive match over `{}`: missing {}",
                            enum_name, rendered
                        ),
                    ));
                }

                return if all_return {
                    Ok(BlockFlow::AlwaysReturns)
                } else {
                    Ok(BlockFlow::FallsThrough)
                };
            }

            let class_scrutinee = match &scrutinee_ty {
                Type::Named(name, _) => self.resolve_class_info(name).is_some(),
                _ => false,
            };
            if class_scrutinee {
                if let Some(pattern) = match_stmt
                    .arms
                    .iter()
                    .map(|arm| &arm.pattern)
                    .find(|pattern| pattern_contains_variant_shape(pattern))
                {
                    return Err(Diagnostic::coded_at(
                        "AU2999",
                        self.pattern_span(pattern),
                        "class patterns are not supported; match an explicit enum/tag representation or use a wildcard and ordinary code",
                    ));
                }
            }

            if !matches!(scrutinee_ty, Type::Tuple(_) | Type::Named(_, _))
                || !(matches!(scrutinee_ty, Type::Tuple(_))
                    || is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                    || is_string_type(&scrutinee_ty)
                    || class_scrutinee)
            {
                return Err(Diagnostic::at(
                match_stmt.span,
                format!(
                    "`match` currently requires a tuple, enum, bool, integer, float, or str scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
            }

            let mut wildcard_span = None;
            let mut all_return = true;
            let mut covered_literals = BTreeMap::<LiteralPatternKey, crate::diag::Span>::new();
            let mut covered_bools = BTreeSet::<bool>::new();
            let mut arm_states = Vec::new();

            for (index, arm) in match_stmt.arms.iter().enumerate() {
                let mut arm_locals = locals.clone();
                if self.is_copy_type(&scrutinee_ty) || pattern_contains_type_arm(&arm.pattern) {
                    if let Some(place) = shared_match_place.as_ref() {
                        self.retain_shared_match_place(place, match_stmt.span, &mut arm_locals);
                    }
                }
                match &arm.pattern {
                    Pattern::Type(_) => unreachable!("root type arms use the typed match checker"),
                    Pattern::Or(pattern) => {
                        self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.capability,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                    Pattern::Wildcard(span) => {
                        if arm.guard.is_none() {
                            if index + 1 != match_stmt.arms.len() {
                                return Err(Diagnostic::at(
                                    *span,
                                    "wildcard match arm must be the final `case`",
                                ));
                            }
                            wildcard_span = Some(*span);
                        }
                    }
                    Pattern::Literal(pattern) => {
                        let key = self.literal_pattern_key(pattern, &scrutinee_ty)?;
                        if let Some(previous) = arm
                            .guard
                            .is_none()
                            .then(|| covered_literals.insert(key.clone(), pattern.span))
                            .flatten()
                        {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                "duplicate match arm for literal `{}` (previously matched at {})",
                                render_literal_pattern_key(&key),
                                previous
                            ),
                            ));
                        }
                        if arm.guard.is_none() {
                            if let LiteralPatternKey::Bool(value) = key {
                                covered_bools.insert(value);
                            }
                        }
                    }
                    Pattern::Variant(pattern) => {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "match over `{}` only supports literal patterns and `_`",
                                scrutinee_ty
                            ),
                        ));
                    }
                    Pattern::Binding(binding) => {
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.capability,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                        if arm.guard.is_none() {
                            if index + 1 != match_stmt.arms.len() {
                                return Err(Diagnostic::at(
                                    binding.span,
                                    "catch-all match arm must be the final `case`",
                                ));
                            }
                            wildcard_span = Some(binding.span);
                        }
                    }
                    Pattern::Tuple(tuple) => {
                        if !matches!(scrutinee_ty, Type::Tuple(_)) {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "tuple pattern requires a tuple scrutinee, found `{}`",
                                    scrutinee_ty
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.capability,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                }

                let prior_patterns = match_stmt.arms[..index]
                    .iter()
                    .filter(|previous_arm| previous_arm.guard.is_none())
                    .map(|previous_arm| &previous_arm.pattern)
                    .collect::<Vec<_>>();
                if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                    return Err(Diagnostic::at(
                        self.pattern_span(&arm.pattern),
                        "unreachable match arm",
                    ));
                }

                self.check_match_guard(
                    arm.guard.as_ref(),
                    &arm.pattern,
                    match_stmt.capability,
                    &mut arm_locals,
                )?;
                self.expire_views_unused_in_branch(&arm.body, &control_last_uses, &mut arm_locals);
                let arm_flow = self.check_block(
                    &arm.body,
                    &mut arm_locals,
                    return_type,
                    loop_depth,
                    allow_return,
                )?;
                if arm_flow == BlockFlow::FallsThrough {
                    all_return = false;
                    arm_states.push(arm_locals);
                }
            }

            if wildcard_span.is_none() {
                if matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                {
                    let missing = [true, false]
                        .into_iter()
                        .filter(|value| !covered_bools.contains(value))
                        .map(|value| format!("`{}`", value))
                        .collect::<Vec<_>>();
                    if !missing.is_empty() {
                        return Err(Diagnostic::at(
                            match_stmt.span,
                            format!(
                                "non-exhaustive match over `bool`: missing {}",
                                missing.join(", ")
                            ),
                        ));
                    }
                } else if matches!(scrutinee_ty, Type::Tuple(_)) {
                    let patterns = match_stmt
                        .arms
                        .iter()
                        .filter(|arm| arm.guard.is_none())
                        .map(|arm| &arm.pattern)
                        .collect::<Vec<_>>();
                    if !self
                        .missing_patterns_for_type(&patterns, &scrutinee_ty)
                        .is_empty()
                    {
                        return Err(Diagnostic::at(
                            match_stmt.span,
                            format!(
                                "non-exhaustive match over `{}`: add a covering tuple pattern or final `case _:`",
                                scrutinee_ty
                            ),
                        ));
                    }
                } else {
                    return Err(Diagnostic::at(
                        match_stmt.span,
                        format!(
                        "`match` over `{}` with literal patterns requires a final `case _:` arm",
                        scrutinee_ty
                    ),
                    ));
                }
            }

            let branch_states = arm_states.iter().collect::<Vec<_>>();
            self.merge_control_flow_moves(locals, &branch_states);

            if all_return {
                Ok(BlockFlow::AlwaysReturns)
            } else {
                Ok(BlockFlow::FallsThrough)
            }
        })();
        self.end_match_borrow_mut(active_match_borrow);
        result.map_err(|error| {
            type_pattern_coverage_diagnostic(
                error,
                match_stmt
                    .arms
                    .iter()
                    .any(|arm| pattern_contains_type_arm(&arm.pattern)),
            )
        })
    }

    pub(super) fn bind_pattern_locals(
        &self,
        pattern: &Pattern,
        expected_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        borrow_mode: ReceiverKind,
        match_borrow_place: Option<&PlacePath>,
        shared_match_scrutinee: Option<&str>,
    ) -> Result<()> {
        match pattern {
            Pattern::Type(pattern) => {
                let member = self.type_pattern_member(pattern, expected_ty)?;
                self.bind_pattern_locals(
                    &Pattern::Binding(pattern.binding.clone()),
                    &member,
                    locals,
                    borrow_mode,
                    match_borrow_place,
                    shared_match_scrutinee,
                )
            }
            Pattern::Variant(pattern)
                if matches!(expected_ty, Type::Union(_) | Type::Unit)
                    && Self::is_none_type_pattern(pattern) =>
            {
                if self
                    .typed_pattern_members(&Pattern::Variant(pattern.clone()), expected_ty)
                    .is_empty()
                {
                    Err(Diagnostic::coded_at(
                        "AU2013",
                        pattern.span,
                        format!("`None` is not a direct member of `{expected_ty}`"),
                    ))
                } else {
                    Ok(())
                }
            }
            Pattern::Or(pattern) => {
                if pattern.alternatives.iter().any(pattern_contains_type_arm) {
                    self.validate_or_pattern_alternatives(pattern, expected_ty)?;
                }
                let original = locals.clone();
                let mut canonical: Option<HashMap<String, LocalBinding>> = None;
                for alternative in &pattern.alternatives {
                    let mut alternative_locals = original.clone();
                    self.bind_pattern_locals(
                        alternative,
                        expected_ty,
                        &mut alternative_locals,
                        borrow_mode,
                        match_borrow_place,
                        shared_match_scrutinee,
                    )?;
                    let added = alternative_locals
                        .iter()
                        .filter(|(name, _)| !original.contains_key(*name))
                        .map(|(name, binding)| (name.clone(), binding.clone()))
                        .collect::<HashMap<_, _>>();
                    if let Some(expected) = &canonical {
                        if expected.len() != added.len()
                            || expected.iter().any(|(name, binding)| {
                                added.get(name).is_none_or(|actual| {
                                    actual.ty != binding.ty || actual.passing != binding.passing
                                })
                            })
                        {
                            return Err(Diagnostic::coded_at(
                                if pattern.alternatives.iter().any(pattern_has_root_type_arm) { "AU2013" } else { "AU2999" },
                                pattern.span,
                                "every alternative in an or-pattern must bind the same names with identical types and capabilities",
                            ));
                        }
                    } else {
                        canonical = Some(added);
                    }
                }
                if let Some(bindings) = canonical {
                    locals.extend(bindings);
                }
                Ok(())
            }
            Pattern::Wildcard(_) => Ok(()),
            Pattern::Literal(pattern) => {
                if matches!(expected_ty, Type::Union(_)) {
                    return Err(Diagnostic::coded_at("AU2013", pattern.span,
                        "a union member literal requires a type arm followed by a guard or nested match"));
                }
                let _ = self.literal_pattern_key(pattern, expected_ty)?;
                Ok(())
            }
            Pattern::Binding(binding) => {
                if locals.contains_key(&binding.name) {
                    return Err(Diagnostic::at(
                        binding.span,
                        format!(
                            "pattern binding `{}` would shadow an existing name",
                            binding.name
                        ),
                    ));
                }
                let passing = if self.is_copy_type(expected_ty) {
                    ReceiverKind::Value
                } else {
                    borrow_mode
                };
                locals.insert(
                    binding.name.clone(),
                    LocalBinding {
                        ty: expected_ty.clone(),
                        assignable: borrow_mode == ReceiverKind::BorrowMut,
                        mutable_place: borrow_mode == ReceiverKind::BorrowMut,
                        managed_resource: false,
                        passing,
                        borrow_origin: None,
                        borrowed_at: (borrow_mode != ReceiverKind::Value).then_some(binding.span),
                        match_borrow_place: (borrow_mode == ReceiverKind::BorrowMut
                            || passing != ReceiverKind::Value)
                            .then(|| match_borrow_place.cloned())
                            .flatten(),
                        stale_match_borrow_place: None,
                        shared_match_scrutinee: (passing == ReceiverKind::Borrow)
                            .then(|| shared_match_scrutinee.map(str::to_string))
                            .flatten(),
                        moved: false,
                        moved_at: None,
                        moved_fields: BTreeMap::new(),
                        frozen_places: BTreeMap::new(),
                        shared_match_places: BTreeMap::new(),
                        captured: false,
                        view: None,
                        closure_loans: Vec::new(),
                        narrowed: BTreeMap::new(),
                        stale_narrowing: BTreeMap::new(),
                    },
                );
                Ok(())
            }
            Pattern::Tuple(tuple_pattern) => {
                if borrow_mode == ReceiverKind::BorrowMut {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        tuple_pattern.span,
                        "`match mut` does not support tuple patterns; bind the tuple as one mutable name",
                    ));
                }
                let Type::Tuple(element_types) = expected_ty else {
                    return Err(Diagnostic::at(
                        tuple_pattern.span,
                        format!(
                            "tuple pattern requires a tuple scrutinee, found `{}`",
                            expected_ty
                        ),
                    ));
                };
                if tuple_pattern.elements.len() != element_types.len() {
                    return Err(Diagnostic::at(
                        tuple_pattern.span,
                        format!(
                            "tuple pattern has {} elements but the scrutinee has {}",
                            tuple_pattern.elements.len(),
                            element_types.len()
                        ),
                    ));
                }
                for (element, element_ty) in tuple_pattern.elements.iter().zip(element_types) {
                    self.bind_pattern_locals(
                        element,
                        element_ty,
                        locals,
                        borrow_mode,
                        match_borrow_place,
                        shared_match_scrutinee,
                    )?;
                }
                Ok(())
            }
            Pattern::Variant(variant_pattern) => {
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "pattern `{}` expects an enum scrutinee, found `{}`",
                            variant_pattern.variant_name, expected_ty
                        ),
                    ));
                };
                let Type::Named(enum_name, _) = expected_ty else {
                    unreachable!("enum pattern scrutinee types should be named");
                };
                let expected_enum_name = self.canonical_enum_name(enum_name);
                let pattern_enum_name = variant_pattern
                    .enum_name
                    .as_deref()
                    .map(|name| self.canonical_enum_name(name))
                    .unwrap_or_else(|| expected_enum_name.clone());
                if pattern_enum_name != expected_enum_name {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "match arm expects enum `{}`, found pattern for `{}`",
                            expected_enum_name, pattern_enum_name
                        ),
                    ));
                }
                let Some((_, payloads)) = variants
                    .iter()
                    .find(|(name, _)| name == &variant_pattern.variant_name)
                else {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "enum `{}` has no variant `{}`",
                            enum_name, variant_pattern.variant_name
                        ),
                    ));
                };
                if variant_pattern.subpatterns.is_empty() && !payloads.is_empty() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` carries a payload and must bind it",
                            expected_enum_name, variant_pattern.variant_name
                        ),
                    ));
                }
                if payloads.is_empty() && !variant_pattern.subpatterns.is_empty() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` does not carry a payload",
                            expected_enum_name, variant_pattern.variant_name
                        ),
                    ));
                }
                if payloads.len() != variant_pattern.subpatterns.len() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` expects {} pattern payload{}, found {}",
                            expected_enum_name,
                            variant_pattern.variant_name,
                            payloads.len(),
                            if payloads.len() == 1 { "" } else { "s" },
                            variant_pattern.subpatterns.len()
                        ),
                    ));
                }
                for (subpattern, payload_ty) in
                    variant_pattern.subpatterns.iter().zip(payloads.iter())
                {
                    self.bind_pattern_locals(
                        subpattern,
                        payload_ty,
                        locals,
                        borrow_mode,
                        match_borrow_place,
                        shared_match_scrutinee,
                    )?;
                }
                Ok(())
            }
        }
    }

    /// Types one `match` arm value the way the surrounding expression uses the
    /// match result.
    pub(super) fn type_of_match_arm_value(
        &self,
        value: &Expr,
        arm_locals: &mut HashMap<String, LocalBinding>,
        result_ty: Option<&Type>,
        result_use: BranchResultUse<'_>,
    ) -> Result<Type> {
        match result_use {
            BranchResultUse::ProjectedField(field) => Ok(self
                .type_member_result_consuming(value, field.name, field.span, arm_locals, result_ty)?
                .0),
            BranchResultUse::Consumed => {
                self.type_expr_consuming_result(value, arm_locals, result_ty)
            }
            BranchResultUse::Inspected => match result_ty {
                Some(expected_ty) => self.type_of_expr_hint(value, arm_locals, Some(expected_ty)),
                None => self.type_of_expr(value, arm_locals),
            },
        }
    }

    pub(super) fn type_of_match_expr(
        &self,
        parts: MatchExprParts<'_>,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
        result_use: BranchResultUse<'_>,
    ) -> Result<Type> {
        let MatchExprParts {
            scrutinee,
            borrow_mode,
            arms,
            span,
        } = parts;
        let shared_scrutinee = self.shared_match_scrutinee_name(scrutinee, borrow_mode);
        let shared_match_place = (borrow_mode == ReceiverKind::Borrow)
            .then(|| self.shared_match_place(scrutinee, locals))
            .flatten();
        let active_match_borrow = if borrow_mode == ReceiverKind::BorrowMut {
            self.begin_match_borrow_mut(scrutinee, span, locals)?
        } else {
            None
        };
        let result = (|| {
            let scrutinee_ty = self.type_of_expr(scrutinee, locals)?;
            let scrutinee_ty = self.narrowed_scrutinee_type(scrutinee, scrutinee_ty, locals);
            if borrow_mode == ReceiverKind::Value {
                if self.is_copy_type(&scrutinee_ty) {
                    if let Some(place) = self.borrow_call_place(scrutinee) {
                        self.ensure_place_not_shared_by_match_for_move(
                            &place,
                            scrutinee.span,
                            locals,
                        )?;
                    }
                } else {
                    self.consume_match_scrutinee_expr(scrutinee, locals)?;
                }
            }
            if arms.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    "`match` requires at least one `case` arm",
                ));
            }

            if matches!(scrutinee_ty, Type::Union(_) | Type::Unit)
                || arms
                    .iter()
                    .any(|arm| pattern_has_root_type_arm(&arm.pattern))
            {
                let mut result_ty = expected.cloned();
                let mut prior = Vec::new();
                let mut arm_states = Vec::new();
                for arm in arms {
                    let mut arm_locals = locals.clone();
                    if let Some(place) = shared_match_place.as_ref() {
                        self.retain_shared_match_place(place, span, &mut arm_locals);
                    }
                    self.bind_pattern_locals(
                        &arm.pattern,
                        &scrutinee_ty,
                        &mut arm_locals,
                        borrow_mode,
                        active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                        shared_scrutinee.as_deref(),
                    )?;
                    if self.patterns_cover_pattern(&prior, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU2013",
                            self.pattern_span(&arm.pattern),
                            "duplicate or unreachable type arm",
                        ));
                    }
                    self.check_match_guard(
                        arm.guard.as_ref(),
                        &arm.pattern,
                        borrow_mode,
                        &mut arm_locals,
                    )?;
                    let arm_ty = self.type_of_match_arm_value(
                        &arm.value,
                        &mut arm_locals,
                        result_ty.as_ref(),
                        result_use,
                    )?;
                    if let Some(expected_ty) = result_ty.as_ref() {
                        if arm_ty != *expected_ty {
                            return Err(Diagnostic::at(arm.value.span,
                                format!("match arm expression expects `{expected_ty}`, found `{arm_ty}`")));
                        }
                    } else {
                        result_ty = Some(arm_ty);
                    }
                    if arm.guard.is_none() {
                        prior.push(&arm.pattern);
                    }
                    arm_states.push(arm_locals);
                }
                let missing = self.missing_patterns_for_type(&prior, &scrutinee_ty);
                if !missing.is_empty() {
                    return Err(Diagnostic::coded_at(
                        "AU2013",
                        span,
                        format!("match does not cover {}", missing.join(", ")),
                    ));
                }
                self.merge_control_flow_moves(locals, &arm_states.iter().collect::<Vec<_>>());
                return Ok(result_ty.expect("nonempty checked match arms"));
            }

            let mut result_ty = expected.cloned();

            if let Some(variants) = self.enum_variants_for_type(&scrutinee_ty) {
                let Type::Named(enum_name, _) = &scrutinee_ty else {
                    unreachable!("enum scrutinee types should be named");
                };
                let scrutinee_enum_name = self.canonical_enum_name(enum_name);
                let mut covered = BTreeSet::<String>::new();
                let mut patterns_by_variant =
                    BTreeMap::<String, Vec<crate::ast::VariantPattern>>::new();
                let mut wildcard_seen = false;
                let mut arm_states = Vec::new();

                for (index, arm) in arms.iter().enumerate() {
                    let mut arm_locals = locals.clone();
                    if self.is_copy_type(&scrutinee_ty) || pattern_contains_type_arm(&arm.pattern) {
                        if let Some(place) = shared_match_place.as_ref() {
                            self.retain_shared_match_place(place, span, &mut arm_locals);
                        }
                    }
                    match &arm.pattern {
                        Pattern::Type(_) => {
                            unreachable!("root type arms use the typed match checker")
                        }
                        Pattern::Or(pattern) => {
                            self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                borrow_mode,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                        Pattern::Wildcard(wildcard_span) => {
                            if arm.guard.is_none() {
                                if index + 1 != arms.len() {
                                    return Err(Diagnostic::at(
                                        *wildcard_span,
                                        "wildcard match arm must be the final `case`",
                                    ));
                                }
                                wildcard_seen = true;
                            }
                        }
                        Pattern::Literal(pattern) => {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                "match over `{}` expects enum variant patterns, not literal `{}`",
                                enum_name,
                                self.render_literal_pattern(pattern)
                            ),
                            ));
                        }
                        Pattern::Binding(binding) => {
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                borrow_mode,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                            if arm.guard.is_none() {
                                if index + 1 != arms.len() {
                                    return Err(Diagnostic::at(
                                        binding.span,
                                        "catch-all match arm must be the final `case`",
                                    ));
                                }
                                wildcard_seen = true;
                            }
                        }
                        Pattern::Tuple(tuple) => {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "match over `{}` expects enum variant patterns, not a tuple pattern",
                                    enum_name
                                ),
                            ));
                        }
                        Pattern::Variant(pattern) => {
                            let pattern_enum_name =
                                if let Some(pattern_enum_name) = &pattern.enum_name {
                                    if pattern_enum_name == enum_name {
                                        pattern_enum_name.clone()
                                    } else if let Some(pattern_enum_info) =
                                        self.resolve_enum_info(pattern_enum_name)
                                    {
                                        self.canonical_enum_info_name(
                                            pattern_enum_name,
                                            pattern_enum_info,
                                        )
                                    } else {
                                        return Err(Diagnostic::at(
                                            pattern.span,
                                            format!(
                                                "unknown enum `{}` in match pattern",
                                                pattern_enum_name
                                            ),
                                        ));
                                    }
                                } else {
                                    scrutinee_enum_name.clone()
                                };
                            if pattern_enum_name != scrutinee_enum_name {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "match arm expects enum `{}`, found pattern for `{}`",
                                        scrutinee_enum_name, pattern_enum_name
                                    ),
                                ));
                            }

                            let Some(variant_payload) = variants
                                .iter()
                                .find(|(name, _)| name == &pattern.variant_name)
                                .map(|(_, payload)| payload.clone())
                            else {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "enum `{}` has no variant `{}`",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            };
                            if arm.guard.is_none()
                                && self.variant_pattern_covers_payloads(pattern, &variant_payload)
                            {
                                covered.insert(pattern.variant_name.clone());
                            }
                            if arm.guard.is_none() {
                                patterns_by_variant
                                    .entry(pattern.variant_name.clone())
                                    .or_default()
                                    .push(pattern.clone());
                            }

                            if pattern.subpatterns.is_empty() && !variant_payload.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` carries a payload and must bind it",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if variant_payload.is_empty() && !pattern.subpatterns.is_empty() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` does not carry a payload",
                                        scrutinee_enum_name, pattern.variant_name
                                    ),
                                ));
                            }
                            if pattern.subpatterns.len() != variant_payload.len() {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "variant `{}.{}` expects {} pattern payload{}, found {}",
                                        scrutinee_enum_name,
                                        pattern.variant_name,
                                        variant_payload.len(),
                                        if variant_payload.len() == 1 { "" } else { "s" },
                                        pattern.subpatterns.len()
                                    ),
                                ));
                            }
                            self.bind_pattern_locals(
                                &arm.pattern,
                                &scrutinee_ty,
                                &mut arm_locals,
                                borrow_mode,
                                active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                    }

                    let prior_patterns = arms[..index]
                        .iter()
                        .filter(|previous_arm| previous_arm.guard.is_none())
                        .map(|previous_arm| &previous_arm.pattern)
                        .collect::<Vec<_>>();
                    if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::at(
                            self.pattern_span(&arm.pattern),
                            "unreachable match arm",
                        ));
                    }

                    self.check_match_guard(
                        arm.guard.as_ref(),
                        &arm.pattern,
                        borrow_mode,
                        &mut arm_locals,
                    )?;
                    let arm_ty = self.type_of_match_arm_value(
                        &arm.value,
                        &mut arm_locals,
                        result_ty.as_ref(),
                        result_use,
                    )?;
                    if let Some(expected_ty) = result_ty.as_ref() {
                        if arm_ty != *expected_ty {
                            if capturing_closure_branch_mismatch(expected_ty, &arm_ty) {
                                return Err(capturing_closure_branch_diagnostic(
                                    "match",
                                    "arm",
                                    arm.value.span,
                                ));
                            }
                            return Err(Diagnostic::at(
                                arm.value.span,
                                format!(
                                    "match arm expression expects `{}`, found `{}`",
                                    expected_ty, arm_ty
                                ),
                            ));
                        }
                    } else {
                        result_ty = Some(arm_ty);
                    }
                    arm_states.push(arm_locals);
                }

                for (variant_name, payloads) in &variants {
                    if covered.contains(variant_name) {
                        continue;
                    }
                    let Some(patterns) = patterns_by_variant.get(variant_name) else {
                        continue;
                    };
                    let pattern_refs = patterns.iter().collect::<Vec<_>>();
                    if self.variant_patterns_cover_payloads_union(&pattern_refs, payloads) {
                        covered.insert(variant_name.clone());
                    }
                }

                let branch_states = arm_states.iter().collect::<Vec<_>>();
                self.merge_control_flow_moves(locals, &branch_states);

                let pattern_refs = arms
                    .iter()
                    .filter(|arm| arm.guard.is_none())
                    .map(|arm| &arm.pattern)
                    .collect::<Vec<_>>();
                let missing = self.missing_patterns_for_type(&pattern_refs, &scrutinee_ty);
                if !wildcard_seen && !missing.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "non-exhaustive match over `{}`: missing {}",
                            enum_name,
                            missing
                                .iter()
                                .map(|name| format!("`{}`", name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ));
                }

                return Ok(result_ty.unwrap_or(Type::Unit));
            }

            let class_scrutinee = match &scrutinee_ty {
                Type::Named(name, _) => self.resolve_class_info(name).is_some(),
                _ => false,
            };
            if class_scrutinee {
                if let Some(pattern) = arms
                    .iter()
                    .map(|arm| &arm.pattern)
                    .find(|pattern| pattern_contains_variant_shape(pattern))
                {
                    return Err(Diagnostic::coded_at(
                        "AU2999",
                        self.pattern_span(pattern),
                        "class patterns are not supported; match an explicit enum/tag representation or use a wildcard and ordinary code",
                    ));
                }
            }

            if !matches!(scrutinee_ty, Type::Tuple(_) | Type::Named(_, _))
                || !(matches!(scrutinee_ty, Type::Tuple(_))
                    || is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                    || is_string_type(&scrutinee_ty)
                    || class_scrutinee)
            {
                return Err(Diagnostic::at(
                span,
                format!(
                    "`match` currently requires a tuple, enum, bool, integer, float, or str scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
            }

            let mut wildcard_seen = false;
            let mut covered_literals = BTreeSet::<LiteralPatternKey>::new();
            let mut covered_bools = BTreeSet::<bool>::new();
            let mut arm_states = Vec::new();

            for (index, arm) in arms.iter().enumerate() {
                let mut arm_locals = locals.clone();
                if self.is_copy_type(&scrutinee_ty) || pattern_contains_type_arm(&arm.pattern) {
                    if let Some(place) = shared_match_place.as_ref() {
                        self.retain_shared_match_place(place, span, &mut arm_locals);
                    }
                }
                match &arm.pattern {
                    Pattern::Type(_) => unreachable!("root type arms use the typed match checker"),
                    Pattern::Or(pattern) => {
                        self.validate_or_pattern_alternatives(pattern, &scrutinee_ty)?;
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                    Pattern::Wildcard(wildcard_span) => {
                        if arm.guard.is_none() {
                            if index + 1 != arms.len() {
                                return Err(Diagnostic::at(
                                    *wildcard_span,
                                    "wildcard match arm must be the final `case`",
                                ));
                            }
                            wildcard_seen = true;
                        }
                    }
                    Pattern::Literal(pattern) => {
                        let key = self.literal_pattern_key(pattern, &scrutinee_ty)?;
                        if arm.guard.is_none() {
                            covered_literals.insert(key.clone());
                            if let LiteralPatternKey::Bool(value) = key {
                                covered_bools.insert(value);
                            }
                        }
                    }
                    Pattern::Variant(pattern) => {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "match over `{}` only supports literal patterns and `_`",
                                scrutinee_ty
                            ),
                        ));
                    }
                    Pattern::Binding(binding) => {
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                        if arm.guard.is_none() {
                            if index + 1 != arms.len() {
                                return Err(Diagnostic::at(
                                    binding.span,
                                    "catch-all match arm must be the final `case`",
                                ));
                            }
                            wildcard_seen = true;
                        }
                    }
                    Pattern::Tuple(tuple) => {
                        if !matches!(scrutinee_ty, Type::Tuple(_)) {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "tuple pattern requires a tuple scrutinee, found `{}`",
                                    scrutinee_ty
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                            active_match_borrow.as_ref().or(shared_match_place.as_ref()),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                }

                let prior_patterns = arms[..index]
                    .iter()
                    .filter(|previous_arm| previous_arm.guard.is_none())
                    .map(|previous_arm| &previous_arm.pattern)
                    .collect::<Vec<_>>();
                if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                    return Err(Diagnostic::at(
                        self.pattern_span(&arm.pattern),
                        "unreachable match arm",
                    ));
                }

                self.check_match_guard(
                    arm.guard.as_ref(),
                    &arm.pattern,
                    borrow_mode,
                    &mut arm_locals,
                )?;
                let arm_ty = self.type_of_match_arm_value(
                    &arm.value,
                    &mut arm_locals,
                    result_ty.as_ref(),
                    result_use,
                )?;
                if let Some(expected_ty) = result_ty.as_ref() {
                    if arm_ty != *expected_ty {
                        if capturing_closure_branch_mismatch(expected_ty, &arm_ty) {
                            return Err(capturing_closure_branch_diagnostic(
                                "match",
                                "arm",
                                arm.value.span,
                            ));
                        }
                        return Err(Diagnostic::at(
                            arm.value.span,
                            format!(
                                "match arm expression expects `{}`, found `{}`",
                                expected_ty, arm_ty
                            ),
                        ));
                    }
                } else {
                    result_ty = Some(arm_ty);
                }
                arm_states.push(arm_locals);
            }

            if matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                && !wildcard_seen
                && covered_bools.len() < 2
            {
                let missing = [false, true]
                    .into_iter()
                    .filter(|value| !covered_bools.contains(value))
                    .map(|value| format!("`{}`", value))
                    .collect::<Vec<_>>();
                return Err(Diagnostic::at(
                    span,
                    format!("non-exhaustive bool match: missing {}", missing.join(", ")),
                ));
            }
            if !wildcard_seen
                && (is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || is_string_type(&scrutinee_ty))
            {
                return Err(Diagnostic::at(
                span,
                format!(
                    "match over `{}` requires a final wildcard arm because the domain is open-ended",
                    scrutinee_ty
                ),
                ));
            }
            if !wildcard_seen && matches!(scrutinee_ty, Type::Tuple(_)) {
                let patterns = arms
                    .iter()
                    .filter(|arm| arm.guard.is_none())
                    .map(|arm| &arm.pattern)
                    .collect::<Vec<_>>();
                if !self
                    .missing_patterns_for_type(&patterns, &scrutinee_ty)
                    .is_empty()
                {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "non-exhaustive match over `{}`: add a covering tuple pattern or final `case _:`",
                            scrutinee_ty
                        ),
                    ));
                }
            }

            let branch_states = arm_states.iter().collect::<Vec<_>>();
            self.merge_control_flow_moves(locals, &branch_states);

            Ok(result_ty.unwrap_or(Type::Unit))
        })();
        self.end_match_borrow_mut(active_match_borrow);
        result.map_err(|error| {
            type_pattern_coverage_diagnostic(
                error,
                arms.iter()
                    .any(|arm| pattern_contains_type_arm(&arm.pattern)),
            )
        })
    }

    pub(super) fn render_literal_pattern(&self, pattern: &LiteralPattern) -> String {
        match &pattern.kind {
            LiteralPatternKind::Int(value) => value.to_string(),
            LiteralPatternKind::Float(value) => value.to_string(),
            LiteralPatternKind::Bool(value) => value.to_string(),
            LiteralPatternKind::String(value) => format!("{:?}", value),
        }
    }

    pub(super) fn literal_pattern_key(
        &self,
        pattern: &LiteralPattern,
        scrutinee_ty: &Type,
    ) -> Result<LiteralPatternKey> {
        match &pattern.kind {
            LiteralPatternKind::Int(value) => {
                let Some(bounds) = integer_type_bounds(scrutinee_ty) else {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                };
                if !value.fits_bounds(bounds) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not fit scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Int(*value))
            }
            LiteralPatternKind::Float(value) => {
                if !is_float_type(scrutinee_ty) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Float(value.to_bits()))
            }
            LiteralPatternKind::Bool(value) => {
                if !matches!(scrutinee_ty, Type::Named(name, args) if name == "bool" && args.is_empty())
                {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Bool(*value))
            }
            LiteralPatternKind::String(value) => {
                if !is_string_type(scrutinee_ty) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern {:?} does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::String(value.clone()))
            }
        }
    }

    pub(super) fn render_variant_pattern_shape(
        &self,
        variant_name: &str,
        payload_tys: &[Type],
    ) -> String {
        if payload_tys.is_empty() {
            return variant_name.to_string();
        }
        let payload = std::iter::repeat_n("_", payload_tys.len())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{variant_name}({payload})")
    }

    pub(super) fn missing_patterns_for_type(
        &self,
        patterns: &[&Pattern],
        expected_ty: &Type,
    ) -> Vec<String> {
        if matches!(expected_ty, Type::Union(_) | Type::Unit) {
            return self.typed_patterns_missing(patterns, expected_ty);
        }
        if patterns
            .iter()
            .any(|pattern| self.pattern_covers_entire_type(pattern, expected_ty))
        {
            return Vec::new();
        }
        if let Type::Tuple(element_types) = expected_ty {
            if self.tuple_patterns_cover_type_union(patterns, element_types) {
                return Vec::new();
            }
            return vec!["_".to_string()];
        }
        let Some(variants) = self.enum_variants_for_type(expected_ty) else {
            return vec!["_".to_string()];
        };
        let mut grouped = BTreeMap::<String, Vec<&VariantPattern>>::new();
        for pattern in patterns {
            let mut pending = vec![*pattern];
            while let Some(pattern) = pending.pop() {
                match pattern {
                    Pattern::Or(or_pattern) => pending.extend(or_pattern.alternatives.iter()),
                    Pattern::Variant(variant_pattern) => {
                        grouped
                            .entry(variant_pattern.variant_name.clone())
                            .or_default()
                            .push(variant_pattern);
                    }
                    _ => {}
                }
            }
        }
        let mut missing = Vec::new();
        for (variant_name, payload_tys) in variants {
            let variant_patterns = grouped.get(&variant_name).cloned().unwrap_or_default();
            if variant_patterns.is_empty() {
                missing.push(self.render_variant_pattern_shape(&variant_name, &payload_tys));
                continue;
            }
            if self.variant_patterns_cover_payloads_union(&variant_patterns, &payload_tys) {
                continue;
            }
            if payload_tys.len() == 1 {
                let nested_patterns = variant_patterns
                    .iter()
                    .filter_map(|pattern| pattern.subpatterns.first())
                    .collect::<Vec<_>>();
                let nested_missing =
                    self.missing_patterns_for_type(&nested_patterns, &payload_tys[0]);
                if !nested_missing.is_empty() {
                    missing.extend(
                        nested_missing
                            .into_iter()
                            .map(|nested| format!("{variant_name}({nested})")),
                    );
                    continue;
                }
            }
            missing.push(self.render_variant_pattern_shape(&variant_name, &payload_tys));
        }
        missing
    }

    pub(super) fn pattern_span(&self, pattern: &Pattern) -> crate::diag::Span {
        match pattern {
            Pattern::Type(pattern) => pattern.span,
            Pattern::Or(pattern) => pattern.span,
            Pattern::Wildcard(span) => *span,
            Pattern::Literal(pattern) => pattern.span,
            Pattern::Binding(binding) => binding.span,
            Pattern::Variant(variant) => variant.span,
            Pattern::Tuple(tuple) => tuple.span,
        }
    }

    pub(super) fn check_match_guard(
        &self,
        guard: Option<&Expr>,
        pattern: &Pattern,
        borrow_mode: ReceiverKind,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let Some(guard) = guard else {
            return Ok(());
        };
        let mut candidate_locals;
        let guard_locals = if borrow_mode == ReceiverKind::Value {
            candidate_locals = locals.clone();
            let mut names = BTreeSet::new();
            Self::collect_pattern_binding_names(pattern, &mut names);
            for name in names {
                if let Some(binding) = candidate_locals.get_mut(&name) {
                    if !self.is_copy_type(&binding.ty) {
                        binding.passing = ReceiverKind::Borrow;
                        binding.borrowed_at = Some(guard.span);
                    }
                }
            }
            &mut candidate_locals
        } else {
            locals
        };
        self.reject_mutable_returned_view_value(guard, guard_locals, false)?;
        let actual = match self.type_of_expr(guard, guard_locals) {
            Ok(actual) => actual,
            Err(mut diagnostic)
                if borrow_mode == ReceiverKind::Value
                    && diagnostic.code == "AU3002"
                    && diagnostic.message.starts_with("cannot move borrowed value") =>
            {
                diagnostic.code = "AU3001".to_string();
                diagnostic.message =
                    "cannot move an owned match candidate before its guard commits the arm"
                        .to_string();
                return Err(diagnostic);
            }
            Err(diagnostic) => return Err(diagnostic),
        };
        let expected = Type::named("bool");
        if actual != expected {
            return Err(Diagnostic::coded_at(
                "AU2002",
                guard.span,
                format!("match guard expects exactly `bool`, found `{actual}`"),
            ));
        }
        Ok(())
    }

    pub(super) fn validate_or_pattern_alternatives(
        &self,
        pattern: &crate::ast::OrPattern,
        expected_ty: &Type,
    ) -> Result<()> {
        let mut prior = Vec::<&Pattern>::new();
        for alternative in &pattern.alternatives {
            if self.patterns_cover_pattern(&prior, alternative, expected_ty) {
                return Err(Diagnostic::coded_at(
                    "AU2999",
                    self.pattern_span(alternative),
                    "duplicate or subsumed alternative in or-pattern",
                ));
            }
            prior.push(alternative);
        }
        Ok(())
    }

    pub(super) fn patterns_cover_pattern(
        &self,
        patterns: &[&Pattern],
        pattern: &Pattern,
        expected_ty: &Type,
    ) -> bool {
        if matches!(expected_ty, Type::Union(_) | Type::Unit) {
            let current = self.typed_pattern_members(pattern, expected_ty);
            let covered = patterns
                .iter()
                .flat_map(|pattern| self.typed_pattern_members(pattern, expected_ty))
                .collect::<BTreeSet<_>>();
            return !current.is_empty() && current.is_subset(&covered);
        }
        match pattern {
            Pattern::Type(_) => self.patterns_cover_type_union(patterns, expected_ty),
            Pattern::Or(or_pattern) => or_pattern
                .alternatives
                .iter()
                .all(|alternative| self.patterns_cover_pattern(patterns, alternative, expected_ty)),
            Pattern::Wildcard(_) | Pattern::Binding(_) => {
                if self.patterns_cover_type_union(patterns, expected_ty) {
                    return true;
                }
                if matches!(expected_ty, Type::Named(name, args) if name == "bool" && args.is_empty())
                {
                    let mut covered = BTreeSet::new();
                    for previous in patterns {
                        match previous {
                            Pattern::Or(or_pattern) => {
                                for alternative in &or_pattern.alternatives {
                                    if let Pattern::Literal(literal) = alternative {
                                        if let Ok(LiteralPatternKey::Bool(value)) =
                                            self.literal_pattern_key(literal, expected_ty)
                                        {
                                            covered.insert(value);
                                        }
                                    }
                                }
                            }
                            Pattern::Wildcard(_) | Pattern::Binding(_) => return true,
                            Pattern::Literal(literal) => {
                                if let Ok(LiteralPatternKey::Bool(value)) =
                                    self.literal_pattern_key(literal, expected_ty)
                                {
                                    covered.insert(value);
                                }
                            }
                            Pattern::Variant(_) => {}
                            Pattern::Tuple(_) => {}
                            Pattern::Type(_) => {
                                if self.pattern_covers_entire_type(previous, expected_ty) {
                                    return true;
                                }
                            }
                        }
                    }
                    return covered.len() == 2;
                }
                patterns
                    .iter()
                    .any(|previous| self.pattern_covers_entire_type(previous, expected_ty))
            }
            Pattern::Variant(current_variant) => {
                if patterns.iter().any(|previous| {
                    self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)
                }) {
                    return true;
                }
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payload_tys)) = variants
                    .iter()
                    .find(|(variant_name, _)| variant_name == &current_variant.variant_name)
                else {
                    return false;
                };
                let variant_patterns = patterns
                    .iter()
                    .filter_map(|previous| match previous {
                        Pattern::Variant(variant)
                            if variant.variant_name == current_variant.variant_name =>
                        {
                            Some(variant)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if current_variant.subpatterns.len() != payload_tys.len() {
                    return false;
                }
                let rows = variant_patterns
                    .iter()
                    .map(|variant| variant.subpatterns.clone())
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_pattern_union(
                    &rows,
                    &current_variant.subpatterns,
                    payload_tys,
                )
            }
            Pattern::Tuple(current_tuple) => {
                if patterns.iter().any(|previous| {
                    self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)
                }) {
                    return true;
                }
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                if current_tuple.elements.len() != element_types.len() {
                    return false;
                }
                let rows = patterns
                    .iter()
                    .filter_map(|previous| match previous {
                        Pattern::Tuple(tuple) if tuple.elements.len() == element_types.len() => {
                            Some(tuple.elements.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_pattern_union(&rows, &current_tuple.elements, element_types)
            }
            Pattern::Literal(_) => patterns
                .iter()
                .any(|previous| self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)),
        }
    }

    pub(super) fn pattern_is_covered_by_pattern(
        &self,
        previous: &Pattern,
        current: &Pattern,
        expected_ty: &Type,
    ) -> bool {
        if matches!(expected_ty, Type::Union(_) | Type::Unit) {
            let current = self.typed_pattern_members(current, expected_ty);
            return !current.is_empty()
                && current.is_subset(&self.typed_pattern_members(previous, expected_ty));
        }
        if self.pattern_covers_entire_type(previous, expected_ty) {
            return true;
        }
        match (previous, current) {
            (Pattern::Or(previous), _) => previous.alternatives.iter().any(|alternative| {
                self.pattern_is_covered_by_pattern(alternative, current, expected_ty)
            }),
            (_, Pattern::Or(current)) => current.alternatives.iter().all(|alternative| {
                self.pattern_is_covered_by_pattern(previous, alternative, expected_ty)
            }),
            (Pattern::Literal(previous), Pattern::Literal(current)) => {
                self.literal_pattern_key(previous, expected_ty).ok()
                    == self.literal_pattern_key(current, expected_ty).ok()
            }
            (Pattern::Variant(previous), Pattern::Variant(current)) => {
                if previous.variant_name != current.variant_name {
                    return false;
                }
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payload_tys)) = variants
                    .iter()
                    .find(|(variant_name, _)| variant_name == &current.variant_name)
                else {
                    return false;
                };
                if previous.subpatterns.len() != payload_tys.len()
                    || current.subpatterns.len() != payload_tys.len()
                {
                    return false;
                }
                previous
                    .subpatterns
                    .iter()
                    .zip(current.subpatterns.iter())
                    .zip(payload_tys.iter())
                    .all(|((previous, current), payload_ty)| {
                        self.pattern_is_covered_by_pattern(previous, current, payload_ty)
                    })
            }
            (Pattern::Tuple(previous), Pattern::Tuple(current)) => {
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                previous.elements.len() == element_types.len()
                    && current.elements.len() == element_types.len()
                    && previous
                        .elements
                        .iter()
                        .zip(&current.elements)
                        .zip(element_types)
                        .all(|((previous, current), ty)| {
                            self.pattern_is_covered_by_pattern(previous, current, ty)
                        })
            }
            _ => false,
        }
    }

    pub(super) fn pattern_covers_entire_type(&self, pattern: &Pattern, expected_ty: &Type) -> bool {
        if matches!(expected_ty, Type::Union(_) | Type::Unit) {
            return self
                .typed_patterns_missing(&[pattern], expected_ty)
                .is_empty();
        }
        match pattern {
            Pattern::Type(pattern) => self.type_pattern_member(pattern, expected_ty).is_ok(),
            Pattern::Or(pattern) => {
                let alternatives = pattern.alternatives.iter().collect::<Vec<_>>();
                self.patterns_cover_type_union(&alternatives, expected_ty)
            }
            Pattern::Wildcard(_) | Pattern::Binding(_) => true,
            Pattern::Literal(_) => false,
            Pattern::Tuple(tuple) => {
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                tuple.elements.len() == element_types.len()
                    && tuple
                        .elements
                        .iter()
                        .zip(element_types)
                        .all(|(element, ty)| self.pattern_covers_entire_type(element, ty))
            }
            Pattern::Variant(variant_pattern) => {
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payloads)) = variants
                    .iter()
                    .find(|(name, _)| name == &variant_pattern.variant_name)
                else {
                    return false;
                };
                if variants.len() != 1 || payloads.len() != variant_pattern.subpatterns.len() {
                    return false;
                }
                variant_pattern.subpatterns.iter().zip(payloads.iter()).all(
                    |(subpattern, payload_ty)| {
                        self.pattern_covers_entire_type(subpattern, payload_ty)
                    },
                )
            }
        }
    }

    pub(super) fn variant_pattern_covers_payloads(
        &self,
        variant_pattern: &VariantPattern,
        payload_tys: &[Type],
    ) -> bool {
        variant_pattern.subpatterns.len() == payload_tys.len()
            && variant_pattern
                .subpatterns
                .iter()
                .zip(payload_tys.iter())
                .all(|(subpattern, payload_ty)| {
                    self.pattern_covers_entire_type(subpattern, payload_ty)
                })
    }

    pub(super) fn patterns_cover_type_union(
        &self,
        patterns: &[&Pattern],
        expected_ty: &Type,
    ) -> bool {
        if matches!(expected_ty, Type::Union(_) | Type::Unit) {
            return self
                .typed_patterns_missing(patterns, expected_ty)
                .is_empty();
        }
        if patterns
            .iter()
            .any(|pattern| self.pattern_covers_entire_type(pattern, expected_ty))
        {
            return true;
        }
        if let Type::Tuple(element_types) = expected_ty {
            return self.tuple_patterns_cover_type_union(patterns, element_types);
        }
        let Some(variants) = self.enum_variants_for_type(expected_ty) else {
            return false;
        };
        let mut grouped = BTreeMap::<String, Vec<&VariantPattern>>::new();
        for pattern in patterns {
            let mut pending = vec![*pattern];
            while let Some(pattern) = pending.pop() {
                match pattern {
                    Pattern::Or(or_pattern) => pending.extend(or_pattern.alternatives.iter()),
                    Pattern::Variant(variant_pattern) => {
                        grouped
                            .entry(variant_pattern.variant_name.clone())
                            .or_default()
                            .push(variant_pattern);
                    }
                    _ => {}
                }
            }
        }
        variants.into_iter().all(|(variant_name, payloads)| {
            let Some(variant_patterns) = grouped.get(&variant_name) else {
                return false;
            };
            self.variant_patterns_cover_payloads_union(variant_patterns, &payloads)
        })
    }

    pub(super) fn tuple_patterns_cover_type_union(
        &self,
        patterns: &[&Pattern],
        element_types: &[Type],
    ) -> bool {
        let rows = patterns
            .iter()
            .flat_map(|pattern| {
                let alternatives: Vec<&Pattern> = match pattern {
                    Pattern::Or(or_pattern) => or_pattern.alternatives.iter().collect(),
                    pattern => vec![*pattern],
                };
                alternatives
                    .into_iter()
                    .filter_map(|pattern| match pattern {
                        Pattern::Tuple(tuple) if tuple.elements.len() == element_types.len() => {
                            Some(tuple.elements.clone())
                        }
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&rows, element_types)
    }

    pub(super) fn pattern_rows_cover_type_union(
        &self,
        rows: &[Vec<Pattern>],
        types: &[Type],
    ) -> bool {
        let Some((first_ty, remaining_types)) = types.split_first() else {
            return !rows.is_empty();
        };
        let irrefutable = |pattern: &Pattern| self.pattern_covers_entire_type(pattern, first_ty);

        if matches!(first_ty, Type::Union(_) | Type::Unit) {
            let members = match first_ty {
                Type::Union(union) => union.members.as_slice(),
                _ => std::slice::from_ref(first_ty),
            };
            return members.iter().all(|member| {
                let key = member.canonical_key(self.module_name, self.canonical_type_names);
                let specialized = rows
                    .iter()
                    .filter_map(|row| {
                        let (first, remaining) = row.split_first()?;
                        self.typed_pattern_members(first, first_ty)
                            .contains(&key)
                            .then(|| remaining.to_vec())
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_type_union(&specialized, remaining_types)
            });
        }

        if matches!(first_ty, Type::Named(name, args) if name == "bool" && args.is_empty()) {
            return [false, true].into_iter().all(|expected| {
                let specialized = rows
                    .iter()
                    .filter_map(|row| {
                        let (first, remaining) = row.split_first()?;
                        if irrefutable(first)
                            || matches!(
                                first,
                                Pattern::Literal(LiteralPattern {
                                    kind: LiteralPatternKind::Bool(actual),
                                    ..
                                }) if *actual == expected
                            )
                        {
                            Some(remaining.to_vec())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_type_union(&specialized, remaining_types)
            });
        }

        if let Type::Tuple(nested_types) = first_ty {
            let mut specialized_types = nested_types.clone();
            specialized_types.extend_from_slice(remaining_types);
            let specialized = rows
                .iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    let mut values = match first {
                        pattern if irrefutable(pattern) => {
                            vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                        }
                        Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                            tuple.elements.clone()
                        }
                        _ => return None,
                    };
                    values.extend_from_slice(remaining);
                    Some(values)
                })
                .collect::<Vec<_>>();
            return self.pattern_rows_cover_type_union(&specialized, &specialized_types);
        }

        if let Some(variants) = self.enum_variants_for_type(first_ty) {
            return variants.into_iter().all(|(variant_name, payload_types)| {
                let mut specialized_types = payload_types.clone();
                specialized_types.extend_from_slice(remaining_types);
                let specialized = rows
                    .iter()
                    .filter_map(|row| {
                        let (first, remaining) = row.split_first()?;
                        let mut values = match first {
                            pattern if irrefutable(pattern) => {
                                vec![
                                    Pattern::Wildcard(self.pattern_span(pattern));
                                    payload_types.len()
                                ]
                            }
                            Pattern::Variant(variant)
                                if variant.variant_name == variant_name
                                    && variant.subpatterns.len() == payload_types.len() =>
                            {
                                variant.subpatterns.clone()
                            }
                            _ => return None,
                        };
                        values.extend_from_slice(remaining);
                        Some(values)
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_type_union(&specialized, &specialized_types)
            });
        }

        let specialized = rows
            .iter()
            .filter_map(|row| {
                let (first, remaining) = row.split_first()?;
                irrefutable(first).then(|| remaining.to_vec())
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&specialized, remaining_types)
    }

    pub(super) fn pattern_rows_cover_pattern_union(
        &self,
        rows: &[Vec<Pattern>],
        current: &[Pattern],
        types: &[Type],
    ) -> bool {
        let Some((first_ty, remaining_types)) = types.split_first() else {
            return current.is_empty() && !rows.is_empty();
        };
        let Some((current_first, current_remaining)) = current.split_first() else {
            return false;
        };
        let irrefutable =
            |pattern: &Pattern| matches!(pattern, Pattern::Wildcard(_) | Pattern::Binding(_));

        let specialize_bool = |expected: bool| {
            rows.iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    if irrefutable(first)
                        || matches!(
                            first,
                            Pattern::Literal(LiteralPattern {
                                kind: LiteralPatternKind::Bool(actual),
                                ..
                            }) if *actual == expected
                        )
                    {
                        Some(remaining.to_vec())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        if matches!(first_ty, Type::Named(name, args) if name == "bool" && args.is_empty()) {
            return match current_first {
                Pattern::Wildcard(_) | Pattern::Binding(_) => {
                    [false, true].into_iter().all(|expected| {
                        self.pattern_rows_cover_pattern_union(
                            &specialize_bool(expected),
                            current_remaining,
                            remaining_types,
                        )
                    })
                }
                Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Bool(expected),
                    ..
                }) => self.pattern_rows_cover_pattern_union(
                    &specialize_bool(*expected),
                    current_remaining,
                    remaining_types,
                ),
                _ => false,
            };
        }

        if let Type::Tuple(nested_types) = first_ty {
            let current_nested = match current_first {
                pattern if irrefutable(pattern) => {
                    vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                }
                Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                    tuple.elements.clone()
                }
                _ => return false,
            };
            let mut specialized_types = nested_types.clone();
            specialized_types.extend_from_slice(remaining_types);
            let mut specialized_current = current_nested;
            specialized_current.extend_from_slice(current_remaining);
            let specialized_rows = rows
                .iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    let mut values = match first {
                        pattern if irrefutable(pattern) => {
                            vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                        }
                        Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                            tuple.elements.clone()
                        }
                        _ => return None,
                    };
                    values.extend_from_slice(remaining);
                    Some(values)
                })
                .collect::<Vec<_>>();
            return self.pattern_rows_cover_pattern_union(
                &specialized_rows,
                &specialized_current,
                &specialized_types,
            );
        }

        if let Some(variants) = self.enum_variants_for_type(first_ty) {
            let current_variant_payloads = match current_first {
                pattern if irrefutable(pattern) => None,
                Pattern::Variant(current_variant) => Some(current_variant.subpatterns.clone()),
                _ => return false,
            };
            let variants_to_cover = if let Pattern::Variant(current_variant) = current_first {
                variants
                    .into_iter()
                    .filter(|(variant_name, payload_types)| {
                        variant_name == &current_variant.variant_name
                            && payload_types.len() == current_variant.subpatterns.len()
                    })
                    .collect()
            } else {
                variants
            };
            if variants_to_cover.is_empty() {
                return false;
            }
            return variants_to_cover
                .into_iter()
                .all(|(variant_name, payload_types)| {
                    let current_payloads = current_variant_payloads.clone().unwrap_or_else(|| {
                        vec![
                            Pattern::Wildcard(self.pattern_span(current_first));
                            payload_types.len()
                        ]
                    });
                    let mut specialized_types = payload_types.clone();
                    specialized_types.extend_from_slice(remaining_types);
                    let mut specialized_current = current_payloads;
                    specialized_current.extend_from_slice(current_remaining);
                    let specialized_rows = rows
                        .iter()
                        .filter_map(|row| {
                            let (first, remaining) = row.split_first()?;
                            let mut values = match first {
                                pattern if irrefutable(pattern) => {
                                    vec![
                                        Pattern::Wildcard(self.pattern_span(pattern));
                                        payload_types.len()
                                    ]
                                }
                                Pattern::Variant(variant)
                                    if variant.variant_name == variant_name
                                        && variant.subpatterns.len() == payload_types.len() =>
                                {
                                    variant.subpatterns.clone()
                                }
                                _ => return None,
                            };
                            values.extend_from_slice(remaining);
                            Some(values)
                        })
                        .collect::<Vec<_>>();
                    self.pattern_rows_cover_pattern_union(
                        &specialized_rows,
                        &specialized_current,
                        &specialized_types,
                    )
                });
        }

        let specialized = rows
            .iter()
            .filter_map(|row| {
                let (first, remaining) = row.split_first()?;
                match current_first {
                    Pattern::Wildcard(_) | Pattern::Binding(_) => {
                        irrefutable(first).then(|| remaining.to_vec())
                    }
                    Pattern::Literal(_) => self
                        .pattern_is_covered_by_pattern(first, current_first, first_ty)
                        .then(|| remaining.to_vec()),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_pattern_union(&specialized, current_remaining, remaining_types)
    }

    pub(super) fn variant_patterns_cover_payloads_union(
        &self,
        patterns: &[&VariantPattern],
        payload_tys: &[Type],
    ) -> bool {
        if patterns
            .iter()
            .any(|pattern| self.variant_pattern_covers_payloads(pattern, payload_tys))
        {
            return true;
        }
        let rows = patterns
            .iter()
            .filter(|pattern| pattern.subpatterns.len() == payload_tys.len())
            .map(|pattern| pattern.subpatterns.clone())
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&rows, payload_tys)
    }
}
