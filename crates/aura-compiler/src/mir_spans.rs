//! Lowering of `Span[T]` slice views (ADR-0061, B1 and B2).
//!
//! A span is a whole-list loan of its base, or a list parameter, plus two
//! hidden `int64` locals: the absolute start of the range and its length.
//! Every span operation runs on the base list with absolute positions. The
//! private `__span_*` list members validate the range against the base on
//! both backends, so a range never reaches past the list it covers.

use super::{
    grouped_mir_expr, CallTarget, Instruction, Lowerer, MirArg, Operand, Rvalue, Terminator,
};
use crate::ast::{Argument, BinaryOp, Expr, ExprKind, ReceiverKind, UnaryOp};
use crate::diag::Span;
use crate::sema::Type;

/// The hidden range of a span: absolute start and length locals.
#[derive(Clone, Debug)]
pub(super) struct SpanRange {
    pub(super) start: String,
    pub(super) len: String,
}

/// A slice view whose base loan is being lowered.
pub(super) enum PendingSpanView {
    /// `view part = items[a:b]`: the bounds are checked once the base loan
    /// exists.
    Slice {
        endpoints: [Operand; 4],
        outer: Option<SpanRange>,
        colon_span: Span,
    },
    /// `view part = f(items)`: the callee checked and returned the range.
    Returned(SpanRange),
}

pub(super) const SPAN_START_FIELD: &str = "__span_start";
pub(super) const SPAN_STOP_FIELD: &str = "__span_stop";
pub(super) const SPAN_INDEX_FIELD: &str = "__span_index";
pub(super) const SPAN_CONTAINS_FIELD: &str = "__span_contains";
pub(super) const SPAN_FIND_FIELD: &str = "__span_find";
pub(super) const SPAN_COUNT_FIELD: &str = "__span_count";
pub(super) const SPAN_EQ_FIELD: &str = "__span_eq";
pub(super) const SPAN_EXTRACT_FIELD: &str = "__span_extract";
pub(super) const SPAN_RESTORE_FIELD: &str = "__span_restore";
pub(super) const SPAN_FILL_FIELD: &str = "__span_fill";

/// The private span members that write the base list.
pub(crate) fn span_member_mutates_receiver(field: &str) -> bool {
    matches!(
        field,
        SPAN_EXTRACT_FIELD | SPAN_RESTORE_FIELD | SPAN_FILL_FIELD
    )
}

/// The hidden keyword-only parameters that carry a `Span[T]` parameter's
/// range.
pub(super) fn span_param_range_names(param: &str) -> (String, String) {
    (
        format!("__aura_span_start_{param}"),
        format!("__aura_span_len_{param}"),
    )
}

pub(super) fn span_type_element(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "Span" && args.len() == 1 => args.first(),
        _ => None,
    }
}

/// A span parameter is a list parameter in MIR.
pub(super) fn span_type_as_list(ty: &Type) -> Type {
    match span_type_element(ty) {
        Some(element) => Type::Named("list".to_string(), vec![element.clone()]),
        None => ty.clone(),
    }
}

fn list_element_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "list" && args.len() == 1 => args.first(),
        _ => None,
    }
}

/// A `-> view Span[T]` function returns its absolute range as
/// `(start, len)`; the caller lends the origin with that range.
pub(super) fn span_range_tuple() -> Type {
    Type::Tuple(vec![int64(), int64()])
}

pub(super) fn span_return_as_range(ty: &Type) -> Type {
    match span_type_element(ty) {
        Some(_) => span_range_tuple(),
        None => ty.clone(),
    }
}

fn int64() -> Type {
    Type::named("int64")
}

fn arg(value: Operand) -> MirArg {
    MirArg {
        name: None,
        value,
        writeback_place: None,
    }
}

fn span_line(span: Span) -> Operand {
    Operand::Int(span.line as u128)
}

fn span_column(span: Span) -> Operand {
    Operand::Int(span.column as u128)
}

impl Lowerer<'_> {
    /// The base place and range of a span-valued expression.
    pub(super) fn span_range_of(&self, expr: &Expr) -> Option<(String, SpanRange)> {
        let ExprKind::Name(name) = &grouped_mir_expr(expr).kind else {
            return None;
        };
        let base = self.render_local_name(name);
        self.span_ranges
            .get(&base)
            .cloned()
            .map(|range| (base, range))
    }

    fn emit_span_member(
        &mut self,
        base: &str,
        field: &str,
        args: Vec<Operand>,
        result: Type,
    ) -> String {
        let target = self.new_typed_temp(result);
        self.emit(Instruction::Assign {
            target: target.clone(),
            value: Rvalue::Call {
                callee: CallTarget::Member {
                    object: Operand::Place(base.to_string()),
                    field: field.to_string(),
                    receiver_place: Some(base.to_string()),
                },
                args: args.into_iter().map(arg).collect(),
                type_args: Default::default(),
            },
        });
        target
    }

    fn span_stop(&mut self, range: &SpanRange, span: Span) -> String {
        let stop = self.new_typed_temp(int64());
        self.emit(Instruction::Assign {
            target: stop.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Add,
                left: Operand::Place(range.start.clone()),
                right: Operand::Place(range.len.clone()),
                span,
                trap: crate::mir::ArithmeticTrap::None,
            },
        });
        stop
    }

    fn materialize_int(&mut self, value: Operand) -> Operand {
        match value {
            Operand::Int(_) => value,
            value => {
                let temp = self.new_typed_temp(int64());
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Use(value),
                });
                Operand::Place(temp)
            }
        }
    }

    /// Endpoint operands of a slice, evaluated once and before the range
    /// loan begins: `(start, has_start, end, has_end)`.
    fn lower_span_endpoints(&mut self, start: Option<&Expr>, end: Option<&Expr>) -> [Operand; 4] {
        let lowered_start = match start {
            Some(start) => {
                let value = self.lower_index_domain_expr(start);
                self.materialize_int(value)
            }
            None => Operand::Int(0),
        };
        let lowered_end = match end {
            Some(end) => {
                let value = self.lower_index_domain_expr(end);
                self.materialize_int(value)
            }
            None => Operand::Int(0),
        };
        [
            lowered_start,
            Operand::Int(u128::from(start.is_some())),
            lowered_end,
            Operand::Int(u128::from(end.is_some())),
        ]
    }

    /// Normalizes slice endpoints against an outer range of `base` (the
    /// whole list when `outer` is `None`) and returns the new range.
    fn emit_span_bounds(
        &mut self,
        base: &str,
        outer: Option<&SpanRange>,
        endpoints: [Operand; 4],
        colon_span: Span,
    ) -> SpanRange {
        let (outer_start, outer_len) = match outer {
            Some(outer) => (
                Operand::Place(outer.start.clone()),
                Operand::Place(outer.len.clone()),
            ),
            None => {
                let len = self.emit_span_member(base, "len", Vec::new(), int64());
                (Operand::Int(0), Operand::Place(len))
            }
        };
        let [start, has_start, end, has_end] = endpoints;
        let args = vec![
            outer_start,
            outer_len,
            start,
            has_start,
            end,
            has_end,
            span_line(colon_span),
            span_column(colon_span),
        ];
        let abs_start = self.emit_span_member(base, SPAN_START_FIELD, args.clone(), int64());
        let abs_stop = self.emit_span_member(base, SPAN_STOP_FIELD, args, int64());
        let len = self.new_typed_temp(int64());
        self.emit(Instruction::Assign {
            target: len.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Sub,
                left: Operand::Place(abs_stop),
                right: Operand::Place(abs_start.clone()),
                span: colon_span,
                trap: crate::mir::ArithmeticTrap::None,
            },
        });
        SpanRange {
            start: abs_start,
            len,
        }
    }

    /// `view name = items[a:b]` or `view name = span[a:b]`: the base place
    /// and then the endpoints are evaluated before the base loan begins. The
    /// view then lowers as a loan or reborrow of the base, and
    /// `finish_span_view` adds the range.
    pub(super) fn begin_span_view(
        &mut self,
        view: &crate::ast::ViewStmt,
    ) -> Option<(crate::ast::ViewStmt, PendingSpanView)> {
        let ExprKind::Slice {
            object,
            start,
            end,
            colon_span,
        } = &grouped_mir_expr(&view.source).kind
        else {
            return None;
        };
        let object = self.pin_span_base(object);
        let endpoints = self.lower_span_endpoints(start.as_deref(), end.as_deref());
        let outer = self.span_range_of(&object).map(|(_, range)| range);
        Some((
            crate::ast::ViewStmt {
                source: object,
                ..view.clone()
            },
            PendingSpanView::Slice {
                endpoints,
                outer,
                colon_span: *colon_span,
            },
        ))
    }

    /// `view part = f(items)` where `f` returns `view Span[T] from items`:
    /// the call returns the absolute range, then the view lends the origin
    /// argument's base place with that range. When the call lent an element
    /// or entry origin through a hidden loan, the view reborrows that loan,
    /// so the origin's selector is not evaluated a second time.
    pub(super) fn begin_returned_span_view(
        &mut self,
        view: &crate::ast::ViewStmt,
    ) -> Option<(crate::ast::ViewStmt, PendingSpanView)> {
        let call = grouped_mir_expr(&view.source);
        let origin = self.returned_span_origin_argument(call)?;
        let origin = match &grouped_mir_expr(&origin).kind {
            ExprKind::Slice { object, .. } => (**object).clone(),
            _ => origin,
        };
        let range = self.lower_span_call_range(call);
        let origin = match self.lent_origin_loan(&origin) {
            Some(loan) => Expr {
                kind: ExprKind::Name(loan),
                span: origin.span,
            },
            None => origin,
        };
        Some((
            crate::ast::ViewStmt {
                source: origin,
                ..view.clone()
            },
            PendingSpanView::Returned(range),
        ))
    }

    /// The caller argument a span-returning call names as its origin.
    fn returned_span_origin_argument(&self, call: &Expr) -> Option<Expr> {
        let ExprKind::Call { callee, args } = &call.kind else {
            return None;
        };
        let info = self.returned_view_callee_decl(callee)?;
        span_type_element(&info.return_type)?;
        let contract = info.decl.view_return.as_ref()?;
        let index = info
            .decl
            .params
            .iter()
            .position(|param| param.name == contract.origin)?;
        let positional = args.iter().take_while(|arg| arg.name.is_none()).count();
        if index < positional {
            return Some(args[index].value.clone());
        }
        args.iter()
            .find(|arg| arg.name.as_deref() == Some(contract.origin.as_str()))
            .map(|arg| arg.value.clone())
    }

    /// The live hidden loan through which a call lent the element or entry
    /// that `origin` names, if the call took one.
    fn lent_origin_loan(&self, origin: &Expr) -> Option<String> {
        [origin, grouped_mir_expr(origin)]
            .into_iter()
            .find_map(|expr| {
                self.contextual_loan_by_span
                    .get(&(expr.span.line, expr.span.column))
            })
            .filter(|loan| self.view_sources.contains_key(*loan))
            .cloned()
    }

    /// Evaluates a span base once, before the slice endpoints, and returns
    /// the base with each evaluated part replaced by the place that holds
    /// it. A root that is not a name, such as a call, is evaluated first.
    /// Element and entry selectors follow, left to right. Lending the
    /// returned place later only reads those places, so the base loan can
    /// begin after the endpoints. A base rooted at a call that returns a
    /// view is left as written, because that call must lower into its loan.
    fn pin_span_base(&mut self, base: &Expr) -> Expr {
        if self.returned_view_source(base).is_some() {
            base.clone()
        } else {
            self.pin_span_place(base)
        }
    }

    fn pin_span_place(&mut self, base: &Expr) -> Expr {
        let kind = match &base.kind {
            ExprKind::Group(inner) => ExprKind::Group(Box::new(self.pin_span_place(inner))),
            ExprKind::Member { object, field } => ExprKind::Member {
                object: Box::new(self.pin_span_place(object)),
                field: field.clone(),
            },
            ExprKind::Index { object, index } => {
                let object = self.pin_span_place(object);
                let index = self.pin_span_selector(&object, index);
                ExprKind::Index {
                    object: Box::new(object),
                    index: Box::new(index),
                }
            }
            ExprKind::Name(_) => return base.clone(),
            // The root value, read the way an index reads its collection.
            _ => match self.lower_expr_at_sequence_point(base, None) {
                Operand::Place(place) | Operand::MovePlace(place) => ExprKind::Name(place),
                value => {
                    let temp = self.new_temp_for_expr(base);
                    self.emit(Instruction::Assign {
                        target: temp.clone(),
                        value: Rvalue::Use(value),
                    });
                    ExprKind::Name(temp)
                }
            },
        };
        Expr {
            kind,
            span: base.span,
        }
    }

    /// A list position or dictionary key, evaluated into a temporary. A
    /// literal stays as written, so element loans keep their literal
    /// selectors, and a tuple position is not a selector.
    fn pin_span_selector(&mut self, object: &Expr, index: &Expr) -> Expr {
        let selector_ty = match self.infer_expr_type(object) {
            Some(Type::Named(name, args)) if name == "list" && args.len() == 1 => int64(),
            Some(Type::Named(name, mut args)) if name == "dict" && args.len() == 2 => {
                args.swap_remove(0)
            }
            _ => return index.clone(),
        };
        if matches!(
            index.kind,
            ExprKind::Int(_) | ExprKind::String(_) | ExprKind::Bool(_)
        ) {
            return index.clone();
        }
        let value = self.lower_expr_with_expected(index, Some(&selector_ty));
        let temp = self.new_typed_temp(selector_ty);
        self.emit(Instruction::Assign {
            target: temp.clone(),
            value: Rvalue::Use(value),
        });
        Expr {
            kind: ExprKind::Name(temp),
            span: index.span,
        }
    }

    /// Calls a span-returning function and reads its `(start, len)` range.
    fn lower_span_call_range(&mut self, call: &Expr) -> SpanRange {
        let value = self.lower_expr(call);
        let tuple = self.new_typed_temp(span_range_tuple());
        self.emit(Instruction::Assign {
            target: tuple.clone(),
            value: Rvalue::Use(value),
        });
        let [start, len] = [0, 1].map(|index| {
            let target = self.new_typed_temp(int64());
            self.emit(Instruction::Assign {
                target: target.clone(),
                value: Rvalue::TupleElement {
                    tuple: Operand::Place(tuple.clone()),
                    index,
                    element_type: int64(),
                },
            });
            target
        });
        SpanRange { start, len }
    }

    /// `return view ...` in a span-returning function yields the absolute
    /// `(start, len)` range of the returned span.
    pub(super) fn lower_returned_span_range(&mut self, value: &Expr) -> Operand {
        let range = match &grouped_mir_expr(value).kind {
            ExprKind::Slice {
                object,
                start,
                end,
                colon_span,
            } => {
                let endpoints = self.lower_span_endpoints(start.as_deref(), end.as_deref());
                let (base, outer) = self.span_base(object);
                self.emit_span_bounds(&base, outer.as_ref(), endpoints, *colon_span)
            }
            ExprKind::Call { .. } => self.lower_span_call_range(value),
            _ => {
                let (base, outer) = self.span_base(value);
                match outer {
                    Some(range) => range,
                    None => self.whole_list_range(&base),
                }
            }
        };
        let temp = self.new_typed_temp(span_range_tuple());
        self.emit(Instruction::Assign {
            target: temp.clone(),
            value: Rvalue::TupleLiteral {
                elements: vec![Operand::Place(range.start), Operand::Place(range.len)],
                element_types: vec![int64(), int64()],
            },
        });
        Operand::Place(temp)
    }

    fn span_base(&mut self, object: &Expr) -> (String, Option<SpanRange>) {
        if let Some((base, range)) = self.span_range_of(object) {
            return (base, Some(range));
        }
        let base = self
            .render_place_expr_option(object)
            .expect("checked returned span origin is a place");
        (base, None)
    }

    fn whole_list_range(&mut self, base: &str) -> SpanRange {
        let len = self.emit_span_member(base, "len", Vec::new(), int64());
        let start = self.new_typed_temp(int64());
        self.emit(Instruction::Assign {
            target: start.clone(),
            value: Rvalue::Use(Operand::Int(0)),
        });
        SpanRange { start, len }
    }

    pub(super) fn finish_span_view(&mut self, base: &str, pending: PendingSpanView) {
        let range = match pending {
            PendingSpanView::Slice {
                endpoints,
                outer,
                colon_span,
            } => self.emit_span_bounds(base, outer.as_ref(), endpoints, colon_span),
            PendingSpanView::Returned(range) => range,
        };
        self.span_ranges.insert(base.to_string(), range);
    }

    /// Maps a span-relative position to an absolute position of the base,
    /// trapping with `AU4003` outside the range.
    pub(super) fn span_absolute_index(
        &mut self,
        object: &Expr,
        relative: Operand,
        span: Span,
    ) -> Operand {
        let Some((base, range)) = self.span_range_of(object) else {
            return relative;
        };
        Operand::Place(self.emit_span_member(
            &base,
            SPAN_INDEX_FIELD,
            vec![
                Operand::Place(range.start),
                Operand::Place(range.len),
                relative,
                span_line(span),
                span_column(span),
            ],
            int64(),
        ))
    }

    /// `span[a:b]` in value position: an owned copy of the subrange.
    pub(super) fn lower_span_value_slice(
        &mut self,
        object: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        colon_span: Span,
        result: Type,
    ) -> Option<Operand> {
        let (base, range) = self.span_range_of(object)?;
        let endpoints = self.lower_span_endpoints(start, end);
        let inner = self.emit_span_bounds(&base, Some(&range), endpoints, colon_span);
        Some(self.emit_span_copy(&base, &inner, colon_span, result))
    }

    fn emit_span_copy(
        &mut self,
        base: &str,
        range: &SpanRange,
        span: Span,
        result: Type,
    ) -> Operand {
        let stop = self.span_stop(range, span);
        Operand::Place(self.emit_span_member(
            base,
            super::INTERNAL_SLICE_FIELD,
            vec![
                Operand::Place(range.start.clone()),
                Operand::Bool(true),
                Operand::Place(stop),
                Operand::Bool(true),
                span_line(span),
                span_column(span),
            ],
            result,
        ))
    }

    /// `len(span)`.
    pub(super) fn lower_span_len(&mut self, object: &Expr) -> Option<Operand> {
        let (_, range) = self.span_range_of(object)?;
        Some(Operand::Place(range.len))
    }

    /// `value in span`.
    pub(super) fn lower_span_membership(
        &mut self,
        value: &Expr,
        container: &Expr,
        negated: bool,
        operator_span: Span,
    ) -> Option<Operand> {
        let (base, range) = self.span_range_of(container)?;
        let element = self
            .local_types
            .get(&base)
            .and_then(list_element_type)
            .cloned();
        let needle = self.lower_expr_at_sequence_point(value, element.as_ref());
        let contains = self.emit_span_member(
            &base,
            SPAN_CONTAINS_FIELD,
            vec![
                Operand::Place(range.start),
                Operand::Place(range.len),
                needle,
            ],
            Type::named("bool"),
        );
        Some(self.maybe_negate(contains, negated, operator_span))
    }

    fn maybe_negate(&mut self, value: String, negated: bool, span: Span) -> Operand {
        if !negated {
            return Operand::Place(value);
        }
        let result = self.new_typed_temp(Type::named("bool"));
        self.emit(Instruction::Assign {
            target: result.clone(),
            value: Rvalue::Unary {
                op: UnaryOp::Not,
                value: Operand::Place(value),
                span,
                trap: crate::mir::ArithmeticTrap::None,
            },
        });
        Operand::Place(result)
    }

    /// `span == other` and `span != other`, where either side is a span and
    /// the other is a span or a list.
    pub(super) fn lower_span_equality(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Option<Operand> {
        if !matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            return None;
        }
        let (span_side, other) = if self.span_range_of(left).is_some() {
            (left, right)
        } else if self.span_range_of(right).is_some() {
            (right, left)
        } else {
            return None;
        };
        let (base, range) = self.span_range_of(span_side)?;
        let (other_value, other_start, other_len) = match self.span_range_of(other) {
            Some((other_base, other_range)) => (
                Operand::Place(other_base),
                Operand::Place(other_range.start),
                Operand::Place(other_range.len),
            ),
            None => {
                let list_ty = self.local_types.get(&base).cloned();
                let value = self.lower_expr_at_sequence_point(other, list_ty.as_ref());
                (value, Operand::Int(0), Operand::Int(u128::MAX))
            }
        };
        let other_len = match other_len {
            Operand::Int(u128::MAX) => {
                // A whole list compares over its own length.
                let negative_one = self.new_typed_temp(int64());
                self.emit(Instruction::Assign {
                    target: negative_one.clone(),
                    value: Rvalue::Binary {
                        op: BinaryOp::Sub,
                        left: Operand::Int(0),
                        right: Operand::Int(1),
                        span,
                        trap: crate::mir::ArithmeticTrap::None,
                    },
                });
                Operand::Place(negative_one)
            }
            other => other,
        };
        let equal = self.emit_span_member(
            &base,
            SPAN_EQ_FIELD,
            vec![
                Operand::Place(range.start),
                Operand::Place(range.len),
                other_value,
                other_start,
                other_len,
            ],
            Type::named("bool"),
        );
        Some(self.maybe_negate(equal, op == BinaryOp::NotEq, span))
    }

    /// A member call on a span receiver. The checker admits only the
    /// closed span member set.
    pub(super) fn lower_span_member_call(
        &mut self,
        call: &Expr,
        object: &Expr,
        field: &str,
        args: &[Argument],
    ) -> Option<Operand> {
        let (base, range) = self.span_range_of(object)?;
        let list_ty = self
            .local_types
            .get(&base)
            .cloned()
            .unwrap_or_else(|| Type::named("Unknown"));
        let element = list_element_type(&list_ty).cloned();
        let start = Operand::Place(range.start.clone());
        let len = Operand::Place(range.len.clone());
        let value = match field {
            "len" => Operand::Place(range.len.clone()),
            "to_list" => self.emit_span_copy(&base, &range, call.span, list_ty),
            "index" | "count" => {
                let needle = self.lower_expr_at_sequence_point(&args[0].value, element.as_ref());
                let member = if field == "index" {
                    SPAN_FIND_FIELD
                } else {
                    SPAN_COUNT_FIELD
                };
                Operand::Place(self.emit_span_member(
                    &base,
                    member,
                    vec![start, len, needle],
                    int64(),
                ))
            }
            "fill" => {
                let fill = self.lower_expr_at_sequence_point(&args[0].value, element.as_ref());
                Operand::Place(self.emit_span_member(
                    &base,
                    SPAN_FILL_FIELD,
                    vec![start, len, fill],
                    Type::Unit,
                ))
            }
            "swap" => {
                let first = self.lower_index_domain_expr(&args[0].value);
                let first = self.materialize_int(first);
                let second = self.lower_index_domain_expr(&args[1].value);
                let second = self.materialize_int(second);
                let first = self.span_absolute_index(object, first, args[0].value.span);
                let second = self.span_absolute_index(object, second, args[1].value.span);
                Operand::Place(self.emit_span_member(
                    &base,
                    "swap",
                    vec![first, second],
                    Type::Unit,
                ))
            }
            "reverse" | "sort" => {
                // Take the range out, run the list operation on it, and put
                // it back. Nothing else can observe the base meanwhile: the
                // span holds it exclusively.
                let taken = self.emit_span_member(
                    &base,
                    SPAN_EXTRACT_FIELD,
                    vec![start.clone(), len],
                    list_ty.clone(),
                );
                let operation = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(Expr {
                            kind: ExprKind::Member {
                                object: Box::new(Expr {
                                    kind: ExprKind::Name(taken.clone()),
                                    span: object.span,
                                }),
                                field: field.to_string(),
                            },
                            span: call.span,
                        }),
                        args: args.to_vec(),
                    },
                    span: call.span,
                };
                let result = self.lower_expr(&operation);
                self.emit(Instruction::Eval { value: result });
                Operand::Place(self.emit_span_member(
                    &base,
                    SPAN_RESTORE_FIELD,
                    vec![start, Operand::MovePlace(taken)],
                    Type::Unit,
                ))
            }
            _ => return None,
        };
        Some(value)
    }

    /// Bounds the shared iteration of a span: `index < start + len`, then
    /// continue in `fetch_block`.
    pub(super) fn emit_span_iteration_bound(
        &mut self,
        stop: &str,
        index: &str,
        fetch_block: usize,
        after_block: usize,
        span: Span,
    ) {
        let more = self.new_typed_temp(Type::named("bool"));
        self.emit(Instruction::Assign {
            target: more.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Less,
                left: Operand::Place(index.to_string()),
                right: Operand::Place(stop.to_string()),
                span,
                trap: crate::mir::ArithmeticTrap::None,
            },
        });
        self.terminate(Terminator::Branch {
            condition: Operand::Place(more),
            then_label: self.label(fetch_block),
            else_label: self.label(after_block),
        });
        self.switch_to(fetch_block);
    }

    /// The start and exclusive stop of a span iterable, if it is one.
    pub(super) fn span_iteration_bounds(&mut self, iterable: &Expr) -> Option<(String, String)> {
        let (_, range) = self.span_range_of(iterable)?;
        let stop = self.span_stop(&range, iterable.span);
        Some((range.start, stop))
    }

    /// The argument for a `Span[T]` parameter: the base place, plus the two
    /// hidden range arguments (ADR-0061, B2). A slice expression is a
    /// call-duration range loan, a list is its whole range, and a span
    /// passes its own range.
    pub(super) fn lower_span_argument(
        &mut self,
        param: &str,
        argument: &Expr,
        expected: &Type,
        passing: Option<ReceiverKind>,
    ) -> (Operand, Vec<MirArg>, Option<String>) {
        let list_ty = span_type_as_list(expected);
        let passing = passing.unwrap_or(ReceiverKind::Borrow);
        let (base_expr, endpoints) = match &grouped_mir_expr(argument).kind {
            ExprKind::Slice {
                object,
                start,
                end,
                colon_span,
            } => {
                let base = self.pin_span_base(object);
                let endpoints = self.lower_span_endpoints(start.as_deref(), end.as_deref());
                (base, Some((endpoints, *colon_span)))
            }
            _ => (argument.clone(), None),
        };
        let base_expr = &base_expr;
        let outer = self.span_range_of(base_expr).map(|(_, range)| range);
        let value = self.lower_expr_for_passing(base_expr, Some(&list_ty), passing);
        let base = match &value {
            Operand::Place(place) | Operand::MovePlace(place) => place.clone(),
            other => {
                let temp = self.new_typed_temp(list_ty.clone());
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Use(other.clone()),
                });
                temp
            }
        };
        let value = match value {
            Operand::Place(_) | Operand::MovePlace(_) => value,
            _ => Operand::Place(base.clone()),
        };
        let writeback_place = (passing == ReceiverKind::BorrowMut)
            .then(|| self.lowered_writeback_place(base_expr, &value))
            .flatten();
        let range = match (endpoints, outer) {
            (Some((endpoints, colon_span)), outer) => {
                self.emit_span_bounds(&base, outer.as_ref(), endpoints, colon_span)
            }
            (None, Some(outer)) => outer,
            (None, None) => self.whole_list_range(&base),
        };
        let (start_name, len_name) = span_param_range_names(param);
        (
            value,
            vec![
                MirArg {
                    name: Some(start_name),
                    value: Operand::Place(range.start),
                    writeback_place: None,
                },
                MirArg {
                    name: Some(len_name),
                    value: Operand::Place(range.len),
                    writeback_place: None,
                },
            ],
            writeback_place,
        )
    }
}
