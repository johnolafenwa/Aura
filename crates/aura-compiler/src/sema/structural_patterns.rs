//! Rest elements, list sequence patterns, and keyword class patterns.

use super::*;
use crate::ast::{ClassPattern, RestPattern, SequencePattern, TuplePattern};

/// The resolved target of one class pattern: the scrutinee member it tests
/// and the specialized type of every stored field it may name.
pub(super) struct ClassPatternTarget {
    /// The class type as the scrutinee spells it (the union member for a
    /// union scrutinee).
    pub member: Type,
    pub fields: Vec<(String, Type)>,
}

pub(super) fn rest_outside_sequence(rest: &RestPattern) -> Diagnostic {
    Diagnostic::coded_at(
        "AU2041",
        rest.span,
        "a `*` rest pattern is allowed only as an element of a tuple or sequence pattern",
    )
}

impl FunctionChecker<'_> {
    /// The class a class pattern names, through a module alias when the
    /// written path uses one.
    pub(super) fn resolve_pattern_class_info(&self, name: &str) -> Option<&ClassInfo> {
        self.resolve_class_info(name).or_else(|| {
            self.canonical_type_names
                .get(name)
                .and_then(|canonical| self.resolve_class_info(canonical))
        })
    }

    /// The positions each tuple pattern element covers, or the arity
    /// diagnostic when the fixed elements do not fit.
    pub(super) fn tuple_pattern_ranges(
        &self,
        tuple: &TuplePattern,
        arity: usize,
    ) -> Result<Vec<std::ops::Range<usize>>> {
        let is_rest = tuple
            .elements
            .iter()
            .map(Pattern::is_rest)
            .collect::<Vec<_>>();
        crate::ast::rest_element_ranges(&is_rest, arity).ok_or_else(|| {
            if is_rest.contains(&true) {
                Diagnostic::coded_at(
                    "AU2041",
                    tuple.span,
                    format!(
                        "tuple pattern has {} element{} besides its rest element but the scrutinee has only {} position{}",
                        tuple.elements.len() - 1,
                        if tuple.elements.len() == 2 { "" } else { "s" },
                        arity,
                        if arity == 1 { "" } else { "s" },
                    ),
                )
            } else {
                Diagnostic::at(
                    tuple.span,
                    format!(
                        "tuple pattern has {} elements but the scrutinee has {}",
                        tuple.elements.len(),
                        arity
                    ),
                )
            }
        })
    }

    /// The tuple pattern's elements with any rest element expanded into one
    /// wildcard per covered position, for coverage analysis. `None` when the
    /// pattern does not fit `arity`.
    pub(super) fn expanded_tuple_elements(
        &self,
        tuple: &TuplePattern,
        arity: usize,
    ) -> Option<Vec<Pattern>> {
        let ranges = self.tuple_pattern_ranges(tuple, arity).ok()?;
        let mut expanded = Vec::with_capacity(arity);
        for (element, range) in tuple.elements.iter().zip(ranges) {
            match element {
                Pattern::Rest(rest) => expanded.extend(std::iter::repeat_n(
                    Pattern::Wildcard(rest.span),
                    range.len(),
                )),
                element => expanded.push(element.clone()),
            }
        }
        Some(expanded)
    }

    /// Binds the elements of a tuple pattern, including a rest element.
    pub(super) fn bind_tuple_pattern_locals(
        &self,
        tuple_pattern: &TuplePattern,
        element_types: &[Type],
        locals: &mut HashMap<String, LocalBinding>,
        borrow_mode: ReceiverKind,
        match_borrow_place: Option<&PlacePath>,
        shared_match_scrutinee: Option<&str>,
    ) -> Result<()> {
        let ranges = self.tuple_pattern_ranges(tuple_pattern, element_types.len())?;
        for (element, range) in tuple_pattern.elements.iter().zip(ranges) {
            match element {
                Pattern::Rest(RestPattern { name: None, .. }) => {}
                Pattern::Rest(RestPattern {
                    name: Some(name),
                    span,
                }) => {
                    if range.is_empty() {
                        return Err(rest_covers_no_positions(*span, name));
                    }
                    self.bind_pattern_locals(
                        &Pattern::Binding(crate::ast::BindingPattern {
                            name: name.clone(),
                            span: *span,
                        }),
                        &Type::Tuple(element_types[range].to_vec()),
                        locals,
                        borrow_mode,
                        match_borrow_place,
                        shared_match_scrutinee,
                    )?;
                }
                element => self.bind_pattern_locals(
                    element,
                    &element_types[range.start],
                    locals,
                    borrow_mode,
                    match_borrow_place,
                    shared_match_scrutinee,
                )?,
            }
        }
        Ok(())
    }

    /// Binds the elements of a list sequence pattern. Only a bare `match`
    /// accepts one; `*rest` binds a fresh owned list copy.
    pub(super) fn bind_sequence_pattern_locals(
        &self,
        sequence: &SequencePattern,
        expected_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        borrow_mode: ReceiverKind,
        match_borrow_place: Option<&PlacePath>,
        shared_match_scrutinee: Option<&str>,
    ) -> Result<()> {
        if borrow_mode != ReceiverKind::Borrow {
            let selector = if borrow_mode == ReceiverKind::BorrowMut {
                "match mut"
            } else {
                "match own"
            };
            return Err(Diagnostic::coded_at(
                "AU2041",
                sequence.span,
                format!("`{selector}` does not support sequence patterns; use a bare `match`"),
            ));
        }
        let Some(element_ty) = vec_element_type(expected_ty) else {
            return Err(Diagnostic::at(
                sequence.span,
                format!("sequence pattern requires a `list[T]` scrutinee, found `{expected_ty}`"),
            ));
        };
        for element in &sequence.elements {
            match element {
                Pattern::Rest(RestPattern { name: None, .. }) => {}
                Pattern::Rest(RestPattern {
                    name: Some(name),
                    span,
                }) => {
                    self.reject_rng_duplication("a sequence rest pattern", element_ty, *span)?;
                    if locals.contains_key(name) {
                        return Err(Diagnostic::at(
                            *span,
                            format!("pattern binding `{name}` would shadow an existing name"),
                        ));
                    }
                    locals.insert(
                        name.clone(),
                        LocalBinding {
                            ty: expected_ty.clone(),
                            assignable: false,
                            mutable_place: true,
                            managed_resource: false,
                            passing: ReceiverKind::Value,
                            borrow_origin: None,
                            borrowed_at: None,
                            match_borrow_place: None,
                            stale_match_borrow_place: None,
                            shared_match_scrutinee: None,
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
                }
                element => self.bind_pattern_locals(
                    element,
                    element_ty,
                    locals,
                    borrow_mode,
                    match_borrow_place,
                    shared_match_scrutinee,
                )?,
            }
        }
        Ok(())
    }

    /// Resolves a class pattern against its scrutinee type: the class, the
    /// union member it tests, and the specialized stored field types.
    pub(super) fn class_pattern_target(
        &self,
        class: &ClassPattern,
        expected_ty: &Type,
    ) -> Result<ClassPatternTarget> {
        let Some(class_info) = self.resolve_pattern_class_info(&class.class_name) else {
            return Err(Diagnostic::coded_at(
                "AU2040",
                class.span,
                format!(
                    "`{}` is not a class; a class pattern names a class, and a variant without a payload is written without parentheses",
                    class.class_name
                ),
            ));
        };
        let identity = (&class_info.module_name, &class_info.decl.name);
        let same_class = |ty: &Type| match ty {
            Type::Named(name, _) => self
                .resolve_class_info(name)
                .is_some_and(|info| (&info.module_name, &info.decl.name) == identity),
            _ => false,
        };
        let member = match expected_ty {
            Type::Union(union) => union.members.iter().find(|member| same_class(member)),
            ty => same_class(ty).then_some(ty),
        };
        let Some(member) = member.cloned() else {
            return Err(Diagnostic::coded_at(
                "AU2040",
                class.span,
                match expected_ty {
                    Type::Union(_) => format!(
                        "class pattern `{}` is not a member of `{expected_ty}`",
                        class.class_name
                    ),
                    _ => format!(
                        "class pattern `{}` does not match scrutinee type `{expected_ty}`",
                        class.class_name
                    ),
                },
            ));
        };
        let mut seen = BTreeSet::new();
        for field in &class.fields {
            if !seen.insert(field.name.as_str()) {
                return Err(Diagnostic::coded_at(
                    "AU2040",
                    field.name_span,
                    format!(
                        "field `{}` appears more than once in class pattern `{}`",
                        field.name, class.class_name
                    ),
                ));
            }
            match class_info.fields.get(&field.name) {
                Some(info) if self.is_external_module(&class_info.module_name) && !info.public => {
                    return Err(Diagnostic::coded_at(
                        "AU2040",
                        field.name_span,
                        format!(
                            "field `{}` is private on `{}`; a class pattern can name it only inside the declaring module",
                            field.name, class_info.decl.name
                        ),
                    ));
                }
                Some(_) => {}
                None if class_info.methods.contains_key(&field.name) => {
                    return Err(Diagnostic::coded_at(
                        "AU2040",
                        field.name_span,
                        format!(
                            "`{}` is a method of `{}`, not a stored field; a class pattern matches stored fields only",
                            field.name, class_info.decl.name
                        ),
                    ));
                }
                None => {
                    return Err(Diagnostic::coded_at(
                        "AU2040",
                        field.name_span,
                        format!(
                            "class `{}` has no field `{}`",
                            class_info.decl.name, field.name
                        ),
                    ));
                }
            }
        }
        let fields = self
            .class_field_types(&member)
            .expect("a class member has stored fields");
        Ok(ClassPatternTarget { member, fields })
    }

    /// The stored fields of a class type in declaration order, specialized
    /// by its type arguments. `None` when the type is not a class.
    pub(super) fn class_field_types(&self, ty: &Type) -> Option<Vec<(String, Type)>> {
        let Type::Named(name, args) = ty else {
            return None;
        };
        let class_info = self.resolve_class_info(name)?;
        let substitutions = substitutions_from_decl_type_args(&class_info.decl.type_params, args);
        Some(
            class_info
                .decl
                .fields
                .iter()
                .filter_map(|decl| {
                    class_info
                        .fields
                        .get(&decl.name)
                        .map(|info| (decl.name.clone(), substitute_type(&info.ty, &substitutions)))
                })
                .collect(),
        )
    }

    /// The field subpatterns of a class pattern as one coverage row, or
    /// `None` when the pattern does not name the class of `ty`.
    pub(super) fn class_pattern_row(
        &self,
        class: &ClassPattern,
        ty: &Type,
    ) -> Option<Vec<Pattern>> {
        let target = self.class_pattern_target(class, ty).ok()?;
        Some(
            Self::class_pattern_rows(class, &target)
                .into_iter()
                .map(|(pattern, _)| pattern)
                .collect(),
        )
    }

    /// Whether the class patterns among `patterns` together match every
    /// value of the class type `ty`, treating the stored fields as columns.
    pub(super) fn class_patterns_cover_class(&self, patterns: &[&Pattern], ty: &Type) -> bool {
        let Some(fields) = self.class_field_types(ty) else {
            return false;
        };
        let mut rows = Vec::new();
        let mut pending = patterns.to_vec();
        while let Some(pattern) = pending.pop() {
            match pattern {
                Pattern::Or(or_pattern) => pending.extend(or_pattern.alternatives.iter()),
                Pattern::Class(class) => rows.extend(self.class_pattern_row(class, ty)),
                _ => {}
            }
        }
        let types = fields.into_iter().map(|(_, ty)| ty).collect::<Vec<_>>();
        !rows.is_empty() && self.pattern_rows_cover_type_union(&rows, &types)
    }

    /// The written subpattern for each declared field, with a wildcard for
    /// an omitted field.
    pub(super) fn class_pattern_rows(
        class: &ClassPattern,
        target: &ClassPatternTarget,
    ) -> Vec<(Pattern, Type)> {
        target
            .fields
            .iter()
            .map(|(name, ty)| {
                let pattern = class
                    .fields
                    .iter()
                    .find(|field| &field.name == name)
                    .map(|field| field.pattern.clone())
                    .unwrap_or(Pattern::Wildcard(class.span));
                (pattern, ty.clone())
            })
            .collect()
    }

    pub(super) fn bind_class_pattern_locals(
        &self,
        class: &ClassPattern,
        expected_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        borrow_mode: ReceiverKind,
        match_borrow_place: Option<&PlacePath>,
        shared_match_scrutinee: Option<&str>,
    ) -> Result<()> {
        if borrow_mode == ReceiverKind::BorrowMut {
            return Err(Diagnostic::coded_at(
                "AU2040",
                class.span,
                "`match mut` does not support class patterns; bind the object as one mutable name and update its fields explicitly",
            ));
        }
        let target = self.class_pattern_target(class, expected_ty)?;
        for field in &class.fields {
            let field_ty = target
                .fields
                .iter()
                .find(|(name, _)| name == &field.name)
                .map(|(_, ty)| ty)
                .expect("checked class pattern field");
            self.bind_pattern_locals(
                &field.pattern,
                field_ty,
                locals,
                borrow_mode,
                match_borrow_place,
                shared_match_scrutinee,
            )?;
        }
        Ok(())
    }

    /// Whether every field subpattern of a class pattern is irrefutable, so
    /// the pattern matches every value of its class.
    pub(super) fn class_pattern_is_irrefutable(&self, class: &ClassPattern, ty: &Type) -> bool {
        let Ok(target) = self.class_pattern_target(class, ty) else {
            return false;
        };
        Self::class_pattern_rows(class, &target)
            .iter()
            .all(|(pattern, field_ty)| self.pattern_covers_entire_type(pattern, field_ty))
    }

    pub(super) fn class_pattern_is_covered_by(
        &self,
        previous: &ClassPattern,
        current: &ClassPattern,
        ty: &Type,
    ) -> bool {
        let (Ok(previous_target), Ok(current_target)) = (
            self.class_pattern_target(previous, ty),
            self.class_pattern_target(current, ty),
        ) else {
            return false;
        };
        if previous_target.member != current_target.member {
            return false;
        }
        Self::class_pattern_rows(previous, &previous_target)
            .iter()
            .zip(Self::class_pattern_rows(current, &current_target))
            .all(|((previous, field_ty), (current, _))| {
                self.pattern_is_covered_by_pattern(previous, &current, field_ty)
            })
    }

    /// The fixed elements before and after the rest element of a sequence
    /// pattern, and whether it has a rest element.
    fn sequence_parts(sequence: &SequencePattern) -> (&[Pattern], &[Pattern], bool) {
        match sequence.elements.iter().position(Pattern::is_rest) {
            Some(index) => (
                &sequence.elements[..index],
                &sequence.elements[index + 1..],
                true,
            ),
            None => (&sequence.elements, &[], false),
        }
    }

    pub(super) fn sequence_pattern_is_covered_by(
        &self,
        previous: &SequencePattern,
        current: &SequencePattern,
        element_ty: &Type,
    ) -> bool {
        let (previous_prefix, previous_suffix, previous_rest) = Self::sequence_parts(previous);
        let (current_prefix, current_suffix, current_rest) = Self::sequence_parts(current);
        let covered = |previous: &[Pattern], current: &[Pattern]| {
            previous.iter().zip(current).all(|(previous, current)| {
                self.pattern_is_covered_by_pattern(previous, current, element_ty)
            })
        };
        if !previous_rest {
            return !current_rest
                && previous_prefix.len() == current_prefix.len()
                && covered(previous_prefix, current_prefix);
        }
        // Every current element outside `previous`'s fixed ends falls in
        // its rest element, which matches anything.
        let current_fixed = current_prefix
            .iter()
            .chain(current_suffix)
            .cloned()
            .collect::<Vec<_>>();
        if current_rest {
            current_prefix.len() >= previous_prefix.len()
                && current_suffix.len() >= previous_suffix.len()
                && covered(previous_prefix, current_prefix)
                && covered(
                    previous_suffix,
                    &current_suffix[current_suffix.len() - previous_suffix.len()..],
                )
        } else {
            current_fixed.len() >= previous_prefix.len() + previous_suffix.len()
                && covered(previous_prefix, &current_fixed)
                && covered(
                    previous_suffix,
                    &current_fixed[current_fixed.len() - previous_suffix.len()..],
                )
        }
    }

    /// Whether a set of sequence patterns matches a list of every length:
    /// irrefutable exact-length arms for each length below the shortest
    /// irrefutable rest arm.
    pub(super) fn sequence_patterns_cover_list(
        &self,
        patterns: &[&Pattern],
        element_ty: &Type,
    ) -> bool {
        let mut exact_lengths = BTreeSet::new();
        let mut shortest_rest = None::<usize>;
        let mut pending = patterns.to_vec();
        while let Some(pattern) = pending.pop() {
            match pattern {
                Pattern::Or(or_pattern) => pending.extend(or_pattern.alternatives.iter()),
                Pattern::Sequence(sequence) => {
                    let (prefix, suffix, rest) = Self::sequence_parts(sequence);
                    if !prefix
                        .iter()
                        .chain(suffix)
                        .all(|element| self.pattern_covers_entire_type(element, element_ty))
                    {
                        continue;
                    }
                    let fixed = prefix.len() + suffix.len();
                    if rest {
                        shortest_rest = Some(shortest_rest.map_or(fixed, |best| best.min(fixed)));
                    } else {
                        exact_lengths.insert(fixed);
                    }
                }
                _ => {}
            }
        }
        shortest_rest.is_some_and(|shortest| (0..shortest).all(|n| exact_lengths.contains(&n)))
    }
}
