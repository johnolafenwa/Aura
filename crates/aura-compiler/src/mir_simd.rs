//! Lowering of `simd` lane loads and stores (ADR-0087).
//!
//! `simd.F64x4.load(xs, i)` and `lanes.store(xs, i)` are ordinary methods
//! of the lane classes in `std/simd.au`: element reads into the lanes, and
//! element writes from them. A call of a function gives the loop around
//! it nothing to reason about, so a direct call lowers here as the element
//! accesses themselves:
//!
//! ```text
//! simd.F64x4.load(xs, i)   =>   simd.F64x4(l0=xs[i], l1=xs[i + 1], ...)
//! lanes.store(xs, i)       =>   xs[i] = lanes.l0; xs[i + 1] = lanes.l1; ...
//! ```
//!
//! `load_array` and `store_array` do the same over a rank-1 `Array`
//! (ADR-0091).
//!
//! The index and the stored value are evaluated once, into temporaries.
//! Both backends, the verifier, and the range proofs of a counted loop then
//! see plain indexing, and an index out of range reports at the call. The
//! methods keep their bodies for every other use, such as a function value.
//! A call whose list is not a local, or that names its arguments, lowers as
//! any other call and runs the method.

use super::{Instruction, Lowerer, Operand, Rvalue};
use crate::ast::{Argument, AssignStmt, AssignTarget, BinaryOp, Expr, ExprKind};
use crate::diag::Span;
use crate::sema::Type;

/// The builtin module that declares the lane classes.
pub(super) const SIMD_MODULE: &str = "simd";

/// The file a trap inside a lane method is reported in. No file has this
/// name, so the report shows the position without a source line.
pub(super) const SIMD_SOURCE_PATH: &str = "<std>/simd.au";

/// How many lanes the lane class `name` holds: `F64x4` holds four.
pub(super) fn simd_lane_count(name: &str) -> Option<usize> {
    let (element, lanes) = name.rsplit_once('x')?;
    if !matches!(element, "F32" | "F64" | "I64") {
        return None;
    }
    lanes
        .parse()
        .ok()
        .filter(|lanes| matches!(lanes, 2 | 4 | 8))
}

impl Lowerer<'_> {
    /// The lanes of the `simd` lane class `class`.
    fn simd_lanes_of(&self, class: &crate::sema::ClassInfo) -> Option<usize> {
        (class.module_name == SIMD_MODULE
            && crate::builtin_sources::has_source_part(&[SIMD_MODULE.to_string()]))
        .then(|| simd_lane_count(&class.decl.name))
        .flatten()
    }

    /// The list argument of a lane load or store, or the Array argument
    /// of an Array load or store, when it is a local.
    fn simd_list_argument<'e>(&self, argument: &'e Argument, holder: &str) -> Option<&'e Expr> {
        let ExprKind::Name(name) = &argument.value.kind else {
            return None;
        };
        let local = self.render_local_name(name);
        matches!(
            self.local_types.get(&local),
            Some(Type::Named(list, _)) if list == holder
        )
        .then_some(&argument.value)
    }

    /// Evaluates `expr` once into a temporary of type `ty` and names it.
    fn simd_temporary(&mut self, expr: &Expr, ty: Type) -> Expr {
        let value = self.lower_expr_with_expected(expr, Some(&ty));
        let temp = self.new_typed_temp(ty);
        self.emit(Instruction::Assign {
            target: temp.clone(),
            value: Rvalue::Use(value),
        });
        Expr {
            kind: ExprKind::Name(temp),
            span: expr.span,
        }
    }

    /// `values[at + lane]`.
    fn simd_element(values: &Expr, at: &Expr, lane: usize, span: Span) -> Expr {
        let index = if lane == 0 {
            at.clone()
        } else {
            Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(at.clone()),
                    right: Box::new(Expr {
                        kind: ExprKind::Int(lane as u128),
                        span,
                    }),
                },
                span,
            }
        };
        Expr {
            kind: ExprKind::Index {
                object: Box::new(values.clone()),
                index: Box::new(index),
            },
            span,
        }
    }

    /// Lowers a direct call of a lane class's `load` or `store` as the
    /// element accesses it stands for (see the module documentation).
    pub(super) fn lower_simd_lane_call(
        &mut self,
        call: &Expr,
        callee: &Expr,
        args: &[Argument],
    ) -> Option<Operand> {
        let ExprKind::Member { object, field } = &callee.kind else {
            return None;
        };
        let [values, at] = args else {
            return None;
        };
        if values.name.is_some() || at.name.is_some() {
            return None;
        }
        let int64 = Type::named("int64");
        let holder = match field.as_str() {
            "load" | "store" => "list",
            _ => "Array",
        };
        match field.as_str() {
            "load" | "load_array" => {
                let (class, _) = self.resolve_associated_method(object, field)?;
                let lanes = self.simd_lanes_of(class)?;
                let values = self.simd_list_argument(values, holder)?.clone();
                let class_ty = self.infer_expr_type(call)?;
                let at = self.simd_temporary(&at.value, int64);
                let construct = Expr {
                    kind: ExprKind::Call {
                        callee: object.clone(),
                        args: (0..lanes)
                            .map(|lane| Argument {
                                name: Some(format!("l{lane}")),
                                value: Self::simd_element(&values, &at, lane, call.span),
                                span: call.span,
                            })
                            .collect(),
                    },
                    span: call.span,
                };
                Some(self.lower_expr_with_expected(&construct, Some(&class_ty)))
            }
            "store" | "store_array" => {
                let receiver_ty = self.infer_expr_type(object)?;
                let Type::Named(class_name, type_args) = &receiver_ty else {
                    return None;
                };
                if !type_args.is_empty() {
                    return None;
                }
                let lanes = self.simd_lanes_of(self.resolve_class_info(class_name)?)?;
                let values = self.simd_list_argument(values, holder)?.clone();
                let stored = self.simd_temporary(object, receiver_ty);
                let at = self.simd_temporary(&at.value, int64);
                for lane in 0..lanes {
                    let ExprKind::Index { object, index } =
                        Self::simd_element(&values, &at, lane, call.span).kind
                    else {
                        unreachable!("a lane element is an index expression")
                    };
                    self.lower_assign(&AssignStmt {
                        mutable: false,
                        target: AssignTarget::Index { object, index },
                        annotation: None,
                        op: None,
                        value: Expr {
                            kind: ExprKind::Member {
                                object: Box::new(stored.clone()),
                                field: format!("l{lane}"),
                            },
                            span: call.span,
                        },
                        span: call.span,
                    });
                }
                Some(Operand::Unit)
            }
            _ => None,
        }
    }
}
