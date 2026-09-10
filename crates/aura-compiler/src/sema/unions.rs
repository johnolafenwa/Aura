//! Expected-boundary injection. The checker selects a member; lowering consumes
//! that decision without re-running literal inference or expression evaluation.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct UnionInjectionId {
    pub module_name: String,
    pub line: usize,
    pub column: usize,
    pub expected_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnionInjection {
    pub union_type: Type,
    pub member_type: Type,
    pub member_index: usize,
}

/// A read (or compound-assignment target) of a union place that the checker
/// proved to hold exactly one member on the current path (ADR-0052 A3/A4).
/// Keyed by the expression span; the lowering reads the payload projection.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NarrowedReadId {
    pub module_name: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NarrowedRead {
    pub union_type: Type,
    pub member_type: Type,
    pub member_index: usize,
}

impl Program {
    pub(crate) fn narrowed_read(
        &self,
        module_name: &str,
        span: crate::diag::Span,
    ) -> Option<NarrowedRead> {
        let id = NarrowedReadId {
            module_name: module_name.to_owned(),
            line: span.line,
            column: span.column,
        };
        self.type_definitions
            .narrowed_reads
            .borrow()
            .get(&id)
            .cloned()
            .or_else(|| {
                find_namespace_in_modules(&self.module_registry, module_name)
                    .or_else(|| find_namespace_in_modules(&self.imported_modules, module_name))
                    .and_then(|namespace| namespace.narrowed_reads.get(&id).cloned())
            })
    }

    pub(crate) fn union_injection(
        &self,
        module_name: &str,
        span: crate::diag::Span,
        expected: &Type,
    ) -> Option<UnionInjection> {
        let id = UnionInjectionId {
            module_name: module_name.to_owned(),
            line: span.line,
            column: span.column,
            expected_key: expected.canonical_key(module_name, &self.canonical_type_names),
        };
        self.type_definitions
            .union_injections
            .borrow()
            .get(&id)
            .cloned()
            .or_else(|| {
                find_namespace_in_modules(&self.module_registry, module_name)
                    .or_else(|| find_namespace_in_modules(&self.imported_modules, module_name))
                    .and_then(|namespace| namespace.union_injections.get(&id).cloned())
            })
    }
}

impl FunctionChecker<'_> {
    /// A borrowed call may borrow a freshly constructed union or a Copy
    /// snapshot. It cannot clone a non-Copy member place to build that union.
    pub(super) fn validate_borrowed_union_injection(
        &self,
        expr: &Expr,
        expected: &Type,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Group(inner) => {
                return self.validate_borrowed_union_injection(inner, expected, locals);
            }
            ExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.validate_borrowed_union_injection(then_expr, expected, locals)?;
                return self.validate_borrowed_union_injection(else_expr, expected, locals);
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    self.validate_borrowed_union_injection(&arm.value, expected, locals)?;
                }
                return Ok(());
            }
            _ => {}
        }
        let id = UnionInjectionId {
            module_name: self.module_name.to_owned(),
            line: expr.span.line,
            column: expr.span.column,
            expected_key: expected.canonical_key(self.module_name, self.canonical_type_names),
        };
        let injection = self.union_injections.borrow().get(&id).cloned();
        if let Some(injection) = injection {
            if !self.is_copy_type(&injection.member_type)
                && self.borrowed_iterable_place(expr, locals)?.is_some()
            {
                return Err(Diagnostic::coded_at(
                    "AU2010",
                    expr.span,
                    format!("borrowed union argument cannot implicitly clone member place of type '{}' into '{expected}'", injection.member_type),
                ).with_help("construct an owned union local first, moving or explicitly cloning the member, then borrow that union"));
            }
        }
        Ok(())
    }

    /// Candidate checks cannot commit moves, loans, obligations, or nested
    /// expression metadata. Alias expansion still charges the compilation budget.
    fn probe_union_member(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        member: &Type,
    ) -> Result<Type> {
        let work = self.type_names.union_probe_work.get();
        if work >= 65_536 {
            return Err(Diagnostic::capacity_at(
                expr.span,
                "union literal probe work exceeds compilation limit of 65536 candidates",
            ));
        }
        self.type_names.union_probe_work.set(work + 1);
        let mut probe = self.clone();
        probe.union_injections = Default::default();
        probe.active_match_borrow_places = Rc::new(RefCell::new(
            self.active_match_borrow_places.borrow().clone(),
        ));
        probe.rng_clone_obligations =
            Rc::new(RefCell::new(self.rng_clone_obligations.borrow().clone()));
        probe.array_equality_obligations = Rc::new(RefCell::new(
            self.array_equality_obligations.borrow().clone(),
        ));
        probe.expr_result_entries =
            Rc::new(RefCell::new(self.expr_result_entries.borrow().clone()));
        probe.closure_infos = Rc::new(RefCell::new(self.closure_infos.borrow().clone()));
        probe.comprehension_infos =
            Rc::new(RefCell::new(self.comprehension_infos.borrow().clone()));
        probe.type_of_expr_hint(expr, &mut locals.clone(), Some(member))
    }

    fn is_union_contextual_literal(expr: &Expr) -> bool {
        matches!(
            expr.kind,
            ExprKind::Int(_)
                | ExprKind::Float(_)
                | ExprKind::Bool(_)
                | ExprKind::String(_)
                | ExprKind::DurationNanos(_)
                | ExprKind::Tuple(_)
                | ExprKind::List(_)
                | ExprKind::Set(_)
                | ExprKind::Map(_)
                | ExprKind::Lambda { .. }
        ) || Self::is_integer_literal_expr(expr)
            || Self::is_float_literal_expr(expr)
            || matches!(&expr.kind, ExprKind::Unary { op: UnaryOp::Neg, expr: inner } if Self::is_float_literal_expr(inner))
            || Self::is_contextual_none_expr(expr)
    }

    pub(super) fn type_union_injection(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        union: &types::UnionType,
    ) -> Result<Type> {
        let expected = Type::Union(Box::new(union.clone()));
        let contextual = Self::is_union_contextual_literal(expr);
        let actual = if contextual {
            if union.members.len() > 128 {
                return Err(Diagnostic::capacity_at(
                    expr.span,
                    "union literal probe capacity exceeded (maximum 128 members)",
                ));
            }
            let mut matches = Vec::new();
            for (index, member) in union.members.iter().enumerate() {
                match self.probe_union_member(expr, locals, member) {
                    Ok(actual) if &actual == member => matches.push(index),
                    Err(error) if error.is_compile_time_capacity() => return Err(error),
                    _ => {}
                }
            }
            if matches.len() > 1 {
                return Err(Diagnostic::coded_at("AU2011", expr.span,
                    format!("literal matches multiple members of '{expected}': {}; annotate one member before injection",
                        matches.iter().map(|index| union.members[*index].to_string()).collect::<Vec<_>>().join(", "))));
            }
            if let Some(index) = matches.first() {
                self.type_of_expr_hint(expr, locals, Some(&union.members[*index]))?
            } else {
                self.type_of_expr(expr, locals).map_err(|error| {
                    if error.code == "AU2002" {
                        Diagnostic::coded_at(
                            "AU2010",
                            expr.span,
                            format!("literal has no matching member of '{expected}'"),
                        )
                        .with_help(error.message)
                    } else {
                        error
                    }
                })?
            }
        } else {
            // In particular, an outer union cannot infer a generic call's
            // arguments backward from its result boundary.
            self.type_of_expr(expr, locals)?
        };
        if actual == expected {
            return Ok(expected);
        }
        let Some(member_index) = union.members.iter().position(|member| member == &actual) else {
            return Err(Diagnostic::coded_at(
                "AU2010",
                expr.span,
                format!("'{actual}' is not a member of '{expected}'"),
            ));
        };
        let id = UnionInjectionId {
            module_name: self.module_name.to_owned(),
            line: expr.span.line,
            column: expr.span.column,
            expected_key: expected.canonical_key(self.module_name, self.canonical_type_names),
        };
        self.union_injections.borrow_mut().insert(
            id,
            UnionInjection {
                union_type: expected.clone(),
                member_type: actual,
                member_index,
            },
        );
        Ok(expected)
    }
}
