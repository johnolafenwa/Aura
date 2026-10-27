//! The math built-ins `abs`, `min`, `max`, `sqrt`, and `round` on unboxed
//! numbers.
//!
//! The runtime helpers behind these functions take and return boxes. When
//! each argument is a scalar local or a numeric literal of a kind the
//! helper accepts, generated code computes the result in words instead and
//! allocates nothing:
//!
//! - `min` and `max` compare and select. Like the helpers, they keep the
//!   first argument when the two compare equal (`a <= b` for `min`,
//!   `a >= b` for `max`) and the second when either is NaN.
//! - `abs` negates a negative integer and clears a float's sign bit. The
//!   absolute value of a signed kind's smallest value does not fit and
//!   stops the program, as unboxing the helper's result did.
//! - `sqrt` of a float and `round` of a float call word helpers.
//!
//! The result has the first argument's layout, as the helpers' results do:
//! a literal first argument is an `int64` or a `float64`, and `round` of a
//! float is an `int64`.

use super::*;

/// One argument of a scalar math call.
#[derive(Clone, Copy, Debug)]
enum MathOperand {
    Local(ScalarKind),
    Int(u128),
    Float,
}

/// How a call's arguments compare.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MathOrder {
    Signed,
    Unsigned,
    Float,
}

impl FunctionCompiler<'_> {
    /// `abs`, `min`, `max`, `sqrt`, or `round` computed in words, or `None`
    /// when an argument is boxed or of a kind only the helper takes.
    pub(super) fn compile_scalar_math(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let operands = args
            .iter()
            .map(|argument| self.math_operand(&argument.value))
            .collect::<Option<Vec<_>>>();
        let Some(operands) = operands else {
            return Ok(None);
        };
        match (op, operands.as_slice()) {
            (FunctionOp::Min | FunctionOp::Max, [first, second]) => {
                let Some((result, order)) = math_comparison(*first, *second) else {
                    return Ok(None);
                };
                let left = self.load_math_operand(&args[0].value, result)?;
                let right = self.load_math_operand(&args[1].value, result)?;
                let less = op == FunctionOp::Min;
                let keep_left = match order {
                    MathOrder::Signed => self.builder.icmp(
                        if less {
                            IntCC::SignedLessThanOrEqual
                        } else {
                            IntCC::SignedGreaterThanOrEqual
                        },
                        left,
                        right,
                    ),
                    MathOrder::Unsigned => self.builder.icmp(
                        if less {
                            IntCC::UnsignedLessThanOrEqual
                        } else {
                            IntCC::UnsignedGreaterThanOrEqual
                        },
                        left,
                        right,
                    ),
                    MathOrder::Float => self.builder.fcmp(
                        if less {
                            FloatCC::LessThanOrEqual
                        } else {
                            FloatCC::GreaterThanOrEqual
                        },
                        left,
                        right,
                    ),
                };
                let chosen = self.builder.select(keep_left, left, right);
                self.math_result(chosen, result).map(Some)
            }
            (FunctionOp::Abs, [MathOperand::Local(kind)]) if kind.is_float() => {
                let value = self.load_math_operand(&args[0].value, *kind)?;
                let bits = self.builder.bitcast(types::I64, value);
                let magnitude = self.builder.band_imm(bits, i64::MAX);
                let value = self.builder.bitcast(types::F64, magnitude);
                self.math_result(value, *kind).map(Some)
            }
            (FunctionOp::Abs, [MathOperand::Local(kind)]) if kind.is_unsigned_integer() => {
                let value = self.load_math_operand(&args[0].value, *kind)?;
                self.math_result(value, *kind).map(Some)
            }
            (FunctionOp::Abs, [MathOperand::Local(kind)]) if kind.is_integer() => {
                let value = self.load_math_operand(&args[0].value, *kind)?;
                Ok(Some(self.compile_signed_abs(value, *kind)))
            }
            (FunctionOp::Sqrt, [MathOperand::Local(kind)]) if kind.is_float() => {
                let value = self.load_math_operand(&args[0].value, *kind)?;
                let root = self.runtime_call_results(self.runtime[RuntimeFn::SqrtF64], &[value]);
                self.math_result(root[0], *kind).map(Some)
            }
            (FunctionOp::Round, [MathOperand::Local(kind)]) if kind.is_float() => {
                let value = self.load_math_operand(&args[0].value, *kind)?;
                let rounded =
                    self.runtime_call_results(self.runtime[RuntimeFn::RoundF64], &[value]);
                self.math_result(rounded[0], ScalarKind::Int64).map(Some)
            }
            // `round` keeps an integer as it is.
            (FunctionOp::Round, [MathOperand::Local(kind)]) if kind.is_integer() => {
                let value = self.load_math_operand(&args[0].value, *kind)?;
                self.math_result(value, *kind).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// What `operand` is, when it is a scalar local or a numeric literal.
    fn math_operand(&self, operand: &Operand) -> Option<MathOperand> {
        match operand {
            Operand::Place(place) | Operand::MovePlace(place) => {
                match self.type_of_place(place).ok()? {
                    DirectType::Scalar(kind) if kind.is_integer() || kind.is_float() => {
                        Some(MathOperand::Local(kind))
                    }
                    _ => None,
                }
            }
            Operand::Int(value) => Some(MathOperand::Int(*value)),
            Operand::Float(_) => Some(MathOperand::Float),
            _ => None,
        }
    }

    /// The word of a math argument whose call's result is of kind `result`.
    fn load_math_operand(
        &mut self,
        operand: &Operand,
        result: ScalarKind,
    ) -> std::result::Result<Val, String> {
        let hint = Some(result).filter(|kind| kind.is_integer());
        let loaded = self.load_operand_with_integer_hint(operand, hint)?;
        Ok(loaded.values[0])
    }

    /// A math result of kind `kind`. A `float32` result is rounded to the
    /// kind, as unboxing the helper's result rounds it.
    fn math_result(
        &mut self,
        value: Val,
        kind: ScalarKind,
    ) -> std::result::Result<ValueRef, String> {
        let value = ValueRef {
            values: vec![value],
            ty: DirectType::Scalar(kind),
        };
        if kind == ScalarKind::Float32 {
            return self.normalize_scalar_value(value);
        }
        Ok(value)
    }

    /// `abs` of a signed integer of kind `kind`. The absolute value of the
    /// kind's smallest value does not fit, and stops the program as
    /// unboxing the helper's result does.
    fn compile_signed_abs(&mut self, value: Val, kind: ScalarKind) -> ValueRef {
        let smallest = kind.narrow_range().map_or(i64::MIN, |(min, _)| min);
        let at_smallest = self.builder.icmp_imm(IntCC::Equal, value, smallest);
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .brif(at_smallest, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.builder.set_cold_block(fail_block);
        // `-i64::MIN` is `2^63`, which the helper reads back unsigned.
        let unsigned = self
            .builder
            .iconst(types::I64, i64::from(kind == ScalarKind::Int64));
        let magnitude = self.builder.iconst(types::I64, smallest.wrapping_neg());
        let kind_word = self.builder.iconst(types::I64, kind.direct_kind_word());
        let (line, column) = self.span_values(None);
        self.builder.call(
            self.runtime[RuntimeFn::FailNarrowOverflow],
            &[magnitude, unsigned, kind_word, line, column],
        );
        self.builder.trap(TrapCode::IntegerOverflow);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        let negative = self.builder.icmp_imm(IntCC::SignedLessThan, value, 0);
        let negated = self.builder.ineg(value);
        ValueRef {
            values: vec![self.builder.select(negative, negated, value)],
            ty: DirectType::Scalar(kind),
        }
    }
}

/// The result kind and comparison of `min` or `max` of `first` and
/// `second`, when both compare as words without a range check. A literal
/// first argument makes an `int64` or a `float64`, as the helper's result
/// does.
fn math_comparison(first: MathOperand, second: MathOperand) -> Option<(ScalarKind, MathOrder)> {
    let result = match first {
        MathOperand::Local(kind) => kind,
        MathOperand::Int(_) => ScalarKind::Int64,
        MathOperand::Float => ScalarKind::Float64,
    };
    let order = if result.is_float() {
        MathOrder::Float
    } else if result == ScalarKind::Uint64 {
        MathOrder::Unsigned
    } else {
        MathOrder::Signed
    };
    let fits = |operand: MathOperand| match (operand, order) {
        (MathOperand::Local(kind), MathOrder::Float) => kind.is_float(),
        (MathOperand::Float, MathOrder::Float) => true,
        (MathOperand::Local(kind), MathOrder::Unsigned) => kind == ScalarKind::Uint64,
        (MathOperand::Int(value), MathOrder::Unsigned) => u64::try_from(value).is_ok(),
        // A word holds a narrow integer's exact value, so an `int64` result
        // takes any integer local but a `uint64` one, and a narrow result
        // only its own kind.
        (MathOperand::Local(kind), MathOrder::Signed) => {
            kind == result || result == ScalarKind::Int64 && kind != ScalarKind::Uint64
        }
        (MathOperand::Int(value), MathOrder::Signed) => {
            let (min, max) = result.narrow_range().unwrap_or((i64::MIN, i64::MAX));
            i64::try_from(value).is_ok_and(|value| (min..=max).contains(&value))
        }
        _ => false,
    };
    (fits(first) && fits(second)).then_some((result, order))
}
