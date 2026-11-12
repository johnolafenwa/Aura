//! Elementwise members, conversion, comparisons, and masks (ADR-0089).
//!
//! Each member gives what a loop over the elements with the scalar
//! function gives. A fault carries the scalar function's diagnostic and
//! the flat index of the first element that failed.

use std::borrow::Cow;

use aura_core::types::Type;

use crate::array_layout::{self, Strides};
use crate::array_simd::{self, KernelFloat};
use crate::diag::{Diagnostic, Result};
use crate::integer::IntegerValue;
use crate::runtime_value::host_builtins::{math_rounding, math_unary, MathFunction};
use crate::runtime_value::{
    array_bool_scalar, array_float32_scalar, array_float64_scalar, array_int32_scalar,
    array_int64_scalar, cast_numeric_value, float_power, mask_error, try_array_buffer, ArrayDType,
    ArrayStorage, ArrayValue, FloatPowerWidth, Value,
};

macro_rules! coded_operations {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $code:literal => $text:literal),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(i64)]
        pub enum $name {
            $($variant = $code),*
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            pub const fn from_code(code: i64) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// The member's name in a program.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|operation| operation.name() == name)
            }
        }
    };
}

coded_operations! {
    /// A member that maps each element of one Array.
    ArrayUnaryOp {
        Abs = 0 => "abs",
        Sqrt = 1 => "sqrt",
        Exp = 2 => "exp",
        Log = 3 => "log",
        Log2 = 4 => "log2",
        Log10 = 5 => "log10",
        Sin = 6 => "sin",
        Cos = 7 => "cos",
        Tan = 8 => "tan",
        Floor = 9 => "floor",
        Ceil = 10 => "ceil",
        Trunc = 11 => "trunc",
        ToFloat = 12 => "to_float",
        LogicalNot = 13 => "logical_not",
    }
}

coded_operations! {
    /// A member that combines each element with the matching element of
    /// an Array that broadcasts, or with one element.
    ArrayPairOp {
        Minimum = 0 => "minimum",
        Maximum = 1 => "maximum",
        Pow = 2 => "pow",
        LogicalAnd = 3 => "logical_and",
        LogicalOr = 4 => "logical_or",
    }
}

coded_operations! {
    /// A comparison member, which gives a mask.
    ArrayCompareOp {
        Less = 0 => "less",
        LessEqual = 1 => "less_equal",
        Greater = 2 => "greater",
        GreaterEqual = 3 => "greater_equal",
        Equal = 4 => "equal",
        NotEqual = 5 => "not_equal",
    }
}

coded_operations! {
    /// A member that reduces a mask to one value.
    ArrayMaskOp {
        Any = 0 => "any",
        All = 1 => "all",
        Count = 2 => "count",
    }
}

/// The diagnostic for a member the Array's dtype does not have. The
/// checker rejects these, so only a dynamic caller sees it.
fn dtype_error(member: &str, expected: &str, found: ArrayDType) -> Diagnostic {
    Diagnostic::coded(
        "AU4007",
        format!(
            "array {member} requires {expected} elements, found {}",
            found.runtime_type_name()
        ),
    )
}

/// `error` for the element at `index`.
fn at_index(mut error: Diagnostic, index: usize) -> Diagnostic {
    error.message = format!("{} at flat index {index}", error.message);
    error
}

/// `convert` of every element, or the first failure with its flat index.
#[inline(always)]
fn convert_elements<T: Copy, U>(
    values: &[T],
    label: &str,
    convert: impl Fn(T) -> Result<U>,
) -> Result<Vec<U>> {
    let mut output = try_array_buffer(values.len(), label)?;
    for (index, value) in values.iter().enumerate() {
        output.push(convert(*value).map_err(|error| at_index(error, index))?);
    }
    Ok(output)
}

/// `convert` of every element, for a conversion that cannot fail.
#[inline(always)]
fn map_elements<T: Copy, U>(values: &[T], label: &str, convert: impl Fn(T) -> U) -> Result<Vec<U>> {
    let mut output = try_array_buffer(values.len(), label)?;
    output.extend(values.iter().map(|value| convert(*value)));
    Ok(output)
}

/// `combine` of each pair of two operands laid out for one result
/// `shape`, in row-major order, for a combination that cannot fail. Runs
/// of adjacent elements zip two slices.
#[inline(always)]
fn zip_elements<T: Copy, U>(
    shape: &[usize],
    left: (&[T], &Strides),
    right: (&[T], &Strides),
    label: &str,
    combine: impl Fn(T, T) -> U,
) -> Result<Vec<U>> {
    let mut output = try_array_buffer(shape.iter().product(), label)?;
    array_layout::walk_runs(
        shape,
        [left.1, right.1],
        |[left_at, right_at], [left_step, right_step], count| {
            match (left_step, right_step) {
                (1, 1) => {
                    let lhs = &left.0[left_at..left_at + count];
                    let rhs = &right.0[right_at..right_at + count];
                    output.extend(lhs.iter().zip(rhs).map(|(lhs, rhs)| combine(*lhs, *rhs)));
                }
                (1, 0) => {
                    let rhs = right.0[right_at];
                    let lhs = &left.0[left_at..left_at + count];
                    output.extend(lhs.iter().map(|lhs| combine(*lhs, rhs)));
                }
                (0, 1) => {
                    let lhs = left.0[left_at];
                    let rhs = &right.0[right_at..right_at + count];
                    output.extend(rhs.iter().map(|rhs| combine(lhs, *rhs)));
                }
                _ => {
                    let (left_at, right_at) = (left_at as isize, right_at as isize);
                    output.extend((0..count as isize).map(|index| {
                        combine(
                            left.0[(left_at + index * left_step) as usize],
                            right.0[(right_at + index * right_step) as usize],
                        )
                    }));
                }
            }
            true
        },
    );
    Ok(output)
}

/// `combine` of each pair, for a combination that can fail. The first
/// failure is reported with its flat index.
fn try_zip_elements<T: Copy, U>(
    shape: &[usize],
    left: (&[T], &Strides),
    right: (&[T], &Strides),
    label: &str,
    combine: impl Fn(T, T) -> Result<U>,
) -> Result<Vec<U>> {
    let mut output = try_array_buffer(shape.iter().product(), label)?;
    let mut outcome = Ok(());
    array_layout::walk_runs(
        shape,
        [left.1, right.1],
        |[left_at, right_at], [left_step, right_step], count| {
            let (left_at, right_at) = (left_at as isize, right_at as isize);
            for index in 0..count as isize {
                let lhs = left.0[(left_at + index * left_step) as usize];
                let rhs = right.0[(right_at + index * right_step) as usize];
                match combine(lhs, rhs) {
                    Ok(value) => output.push(value),
                    Err(error) => {
                        outcome = Err(at_index(error, output.len()));
                        return false;
                    }
                }
            }
            true
        },
    );
    outcome.map(|()| output)
}

/// The smaller of two floats as `min` orders them: a NaN wins, and
/// `-0.0` is below `0.0`.
#[inline(always)]
fn float_minimum<T: KernelFloat>(left: T, right: T) -> T {
    if left.is_nan_value() {
        left
    } else if right.is_nan_value() {
        right
    } else if left < right || (left.is_negative_zero() && right.is_positive_zero()) {
        left
    } else {
        right
    }
}

/// The larger of two floats as `max` orders them.
#[inline(always)]
fn float_maximum<T: KernelFloat>(left: T, right: T) -> T {
    if left.is_nan_value() {
        left
    } else if right.is_nan_value() {
        right
    } else if left > right || (left.is_positive_zero() && right.is_negative_zero()) {
        left
    } else {
        right
    }
}

#[inline(always)]
/// Appends `count` elements of one run of `choose`: each comes from
/// `when_true` where the mask holds `true` and from `when_false`
/// elsewhere. An operand is its elements, the position of the run's first
/// element, and the step between elements, which is zero for one element
/// repeated. A run that walks the mask forward, with each side either
/// forward or repeated, is a loop over slices with no index arithmetic.
fn choose_run<T: Copy>(
    output: &mut Vec<T>,
    (mask, mask_at, mask_step): (&[bool], usize, isize),
    (when_true, true_at, true_step): (&[T], usize, isize),
    (when_false, false_at, false_step): (&[T], usize, isize),
    count: usize,
) {
    let pick = |flag: bool, chosen: T, otherwise: T| if flag { chosen } else { otherwise };
    if mask_step == 1 && matches!(true_step, 0 | 1) && matches!(false_step, 0 | 1) {
        let mask = &mask[mask_at..mask_at + count];
        fn forward<T>(values: &[T], at: usize, count: usize) -> &[T] {
            &values[at..at + count]
        }
        match (true_step, false_step) {
            (1, 1) => {
                let (chosen, otherwise) = (
                    forward(when_true, true_at, count),
                    forward(when_false, false_at, count),
                );
                output.extend(
                    mask.iter()
                        .zip(chosen)
                        .zip(otherwise)
                        .map(|((flag, chosen), otherwise)| pick(*flag, *chosen, *otherwise)),
                );
            }
            (1, _) => {
                let otherwise = when_false[false_at];
                output.extend(
                    mask.iter()
                        .zip(forward(when_true, true_at, count))
                        .map(|(flag, chosen)| pick(*flag, *chosen, otherwise)),
                );
            }
            (_, 1) => {
                let chosen = when_true[true_at];
                output.extend(
                    mask.iter()
                        .zip(forward(when_false, false_at, count))
                        .map(|(flag, otherwise)| pick(*flag, chosen, *otherwise)),
                );
            }
            _ => {
                let (chosen, otherwise) = (when_true[true_at], when_false[false_at]);
                output.extend(mask.iter().map(|flag| pick(*flag, chosen, otherwise)));
            }
        }
        return;
    }
    let at =
        |position: usize, step: isize, index: isize| (position as isize + index * step) as usize;
    output.extend((0..count as isize).map(|index| {
        pick(
            mask[at(mask_at, mask_step, index)],
            when_true[at(true_at, true_step, index)],
            when_false[at(false_at, false_step, index)],
        )
    }));
}

/// A layout that repeats one element over every axis of `shape`.
fn repeated(shape: &[usize]) -> Strides {
    Strides {
        offset: 0,
        strides: vec![0; shape.len()].into(),
    }
}

/// The second operand of a pairwise member: an Array of the receiver's
/// dtype, or one element.
struct Operand<'a> {
    array: Option<&'a ArrayValue>,
    scalar: &'a Value,
}

impl ArrayValue {
    /// The operand `other` and the shape it broadcasts to with `self`.
    fn pair_operand<'a>(
        &self,
        member: &str,
        other: &'a Value,
    ) -> Result<(Operand<'a>, Vec<usize>)> {
        let Value::Array(array) = other else {
            return Ok((
                Operand {
                    array: None,
                    scalar: other,
                },
                self.shape.to_vec(),
            ));
        };
        let mismatch = || {
            Diagnostic::coded(
                "AU4007",
                format!(
                    "array {member} requires the same dtype and shapes that broadcast, found {:?} {} and {:?} {}",
                    self.shape,
                    self.dtype().runtime_type_name(),
                    array.shape,
                    array.dtype().runtime_type_name()
                ),
            )
        };
        if self.dtype() != array.dtype() {
            return Err(mismatch());
        }
        let shape =
            array_layout::broadcast_shape(&self.shape, &array.shape).ok_or_else(mismatch)?;
        Ok((
            Operand {
                array: Some(array),
                scalar: other,
            },
            shape,
        ))
    }

    /// One member of [`ArrayUnaryOp`] applied to every element.
    pub fn unary(&self, operation: ArrayUnaryOp) -> Result<Self> {
        use ArrayUnaryOp as Op;
        let name = operation.name();
        let label = "array elementwise result";
        let elements = self.elements()?;
        let dtype = self.dtype();
        let math = |function: MathFunction| match &*elements {
            ArrayStorage::Float64(values) => Ok(ArrayStorage::Float64(
                convert_elements(values, label, |value| math_unary(function, value))?.into(),
            )),
            _ => Err(dtype_error(name, "float64", dtype)),
        };
        let rounding = |function: MathFunction| match &*elements {
            ArrayStorage::Float64(values) => Ok(ArrayStorage::Int64(
                convert_elements(values, label, |value| math_rounding(function, value))?.into(),
            )),
            _ => Err(dtype_error(name, "float64", dtype)),
        };
        let storage = match operation {
            Op::Abs => {
                let overflow = || Diagnostic::coded("AU4002", "array abs overflowed".to_string());
                match &*elements {
                    ArrayStorage::Int32(values) => ArrayStorage::Int32(
                        convert_elements(values, label, |value| {
                            value.checked_abs().ok_or_else(overflow)
                        })?
                        .into(),
                    ),
                    ArrayStorage::Int64(values) => ArrayStorage::Int64(
                        convert_elements(values, label, |value| {
                            value.checked_abs().ok_or_else(overflow)
                        })?
                        .into(),
                    ),
                    ArrayStorage::Float32(values) => {
                        ArrayStorage::Float32(map_elements(values, label, f32::abs)?.into())
                    }
                    ArrayStorage::Float64(values) => {
                        ArrayStorage::Float64(map_elements(values, label, f64::abs)?.into())
                    }
                    ArrayStorage::Bool(_) => return Err(mask_error(name)),
                }
            }
            Op::Sqrt => match &*elements {
                ArrayStorage::Float64(values) => ArrayStorage::Float64(
                    array_simd::widest(
                        #[inline(always)]
                        || map_elements(values, label, f64::sqrt),
                    )?
                    .into(),
                ),
                _ => return Err(dtype_error(name, "float64", dtype)),
            },
            Op::Exp => math(MathFunction::Exp)?,
            Op::Log => math(MathFunction::Log)?,
            Op::Log2 => math(MathFunction::Log2)?,
            Op::Log10 => math(MathFunction::Log10)?,
            Op::Sin => math(MathFunction::Sin)?,
            Op::Cos => math(MathFunction::Cos)?,
            Op::Tan => math(MathFunction::Tan)?,
            Op::Floor => rounding(MathFunction::Floor)?,
            Op::Ceil => rounding(MathFunction::Ceil)?,
            Op::Trunc => rounding(MathFunction::Trunc)?,
            Op::ToFloat => match &*elements {
                ArrayStorage::Int32(values) => {
                    ArrayStorage::Float64(map_elements(values, label, f64::from)?.into())
                }
                ArrayStorage::Int64(values) => {
                    ArrayStorage::Float64(map_elements(values, label, |value| value as f64)?.into())
                }
                _ => return Err(dtype_error(name, "integer", dtype)),
            },
            Op::LogicalNot => match &*elements {
                ArrayStorage::Bool(values) => {
                    ArrayStorage::Bool(map_elements(values, label, |value: bool| !value)?.into())
                }
                _ => return Err(dtype_error(name, "bool", dtype)),
            },
        };
        Self::new(self.try_shape()?, storage)
    }

    /// One member of [`ArrayPairOp`] over `self` and `other`, an Array of
    /// the same dtype that broadcasts or one element.
    pub fn pairwise(&self, other: &Value, operation: ArrayPairOp) -> Result<Self> {
        use ArrayPairOp as Op;
        let name = operation.name();
        let label = "array elementwise result";
        let (operand, shape) = self.pair_operand(name, other)?;
        let left_layout = array_layout::broadcast_layout(&self.shape, &self.layout(), &shape);
        let scalar_layout = repeated(&shape);
        macro_rules! operands {
            ($variant:ident, $values:ident, $convert:ident, |$left:ident, $right:ident| $body:expr) => {{
                let scalar;
                let right_layout;
                let $left = (&**$values, &left_layout);
                let $right = match operand.array {
                    Some(array) => {
                        let ArrayStorage::$variant(elements) = &array.base else {
                            unreachable!("matching array dtypes imply matching storage variants")
                        };
                        right_layout =
                            array_layout::broadcast_layout(&array.shape, &array.layout(), &shape);
                        (&**elements, &right_layout)
                    }
                    None => {
                        scalar = [$convert(operand.scalar, 0)?];
                        (&scalar[..], &scalar_layout)
                    }
                };
                ArrayStorage::$variant($body?.into())
            }};
        }
        let storage = match (operation, &self.base) {
            (Op::Minimum | Op::Maximum, ArrayStorage::Int32(values)) => {
                let smaller = operation == Op::Minimum;
                operands!(Int32, values, array_int32_scalar, |left, right| {
                    zip_elements(&shape, left, right, label, |lhs: i32, rhs| {
                        if smaller {
                            lhs.min(rhs)
                        } else {
                            lhs.max(rhs)
                        }
                    })
                })
            }
            (Op::Minimum | Op::Maximum, ArrayStorage::Int64(values)) => {
                let smaller = operation == Op::Minimum;
                operands!(Int64, values, array_int64_scalar, |left, right| {
                    zip_elements(&shape, left, right, label, |lhs: i64, rhs| {
                        if smaller {
                            lhs.min(rhs)
                        } else {
                            lhs.max(rhs)
                        }
                    })
                })
            }
            (Op::Minimum | Op::Maximum, ArrayStorage::Float32(values)) => {
                let smaller = operation == Op::Minimum;
                operands!(Float32, values, array_float32_scalar, |left, right| {
                    zip_elements(&shape, left, right, label, |lhs: f32, rhs| {
                        if smaller {
                            float_minimum(lhs, rhs)
                        } else {
                            float_maximum(lhs, rhs)
                        }
                    })
                })
            }
            (Op::Minimum | Op::Maximum, ArrayStorage::Float64(values)) => {
                let smaller = operation == Op::Minimum;
                operands!(Float64, values, array_float64_scalar, |left, right| {
                    zip_elements(&shape, left, right, label, |lhs: f64, rhs| {
                        if smaller {
                            float_minimum(lhs, rhs)
                        } else {
                            float_maximum(lhs, rhs)
                        }
                    })
                })
            }
            (Op::Pow, ArrayStorage::Float64(values)) => {
                operands!(Float64, values, array_float64_scalar, |left, right| {
                    try_zip_elements(&shape, left, right, label, |lhs, rhs| {
                        float_power(lhs, rhs, FloatPowerWidth::Float64)
                    })
                })
            }
            (Op::LogicalAnd | Op::LogicalOr, ArrayStorage::Bool(values)) => {
                let both = operation == Op::LogicalAnd;
                operands!(Bool, values, array_bool_scalar, |left, right| zip_elements(
                    &shape,
                    left,
                    right,
                    label,
                    |lhs: bool, rhs| if both { lhs & rhs } else { lhs | rhs }
                ))
            }
            (Op::Minimum | Op::Maximum, ArrayStorage::Bool(_)) => return Err(mask_error(name)),
            (Op::Pow, _) => return Err(dtype_error(name, "float64", self.dtype())),
            (Op::LogicalAnd | Op::LogicalOr, _) => {
                return Err(dtype_error(name, "bool", self.dtype()));
            }
        };
        Self::new(shape.into(), storage)
    }

    /// The mask of one comparison of `self` with `other`, an Array of the
    /// same dtype that broadcasts or one element.
    pub fn compare(&self, other: &Value, operation: ArrayCompareOp) -> Result<Self> {
        let name = operation.name();
        let label = "array comparison result";
        let (operand, shape) = self.pair_operand(name, other)?;
        let left_layout = array_layout::broadcast_layout(&self.shape, &self.layout(), &shape);
        let scalar_layout = repeated(&shape);
        macro_rules! per_dtype {
            ($(($variant:ident, $convert:ident)),*) => {
                match &self.base {
                    $(ArrayStorage::$variant(values) => {
                        let scalar;
                        let right_layout;
                        let right = match operand.array {
                            Some(array) => {
                                let ArrayStorage::$variant(elements) = &array.base else {
                                    unreachable!(
                                        "matching array dtypes imply matching storage variants"
                                    )
                                };
                                right_layout = array_layout::broadcast_layout(
                                    &array.shape,
                                    &array.layout(),
                                    &shape,
                                );
                                (&**elements, &right_layout)
                            }
                            None => {
                                scalar = [$convert(operand.scalar, 0)?];
                                (&scalar[..], &scalar_layout)
                            }
                        };
                        // One loop for each operation, so the loop holds
                        // one comparison and no branch on the operation.
                        macro_rules! mask_of {
                            ($holds:expr) => {
                                array_simd::widest(
                                    #[inline(always)]
                                    || {
                                        zip_elements(
                                            &shape,
                                            (&**values, &left_layout),
                                            right,
                                            label,
                                            $holds,
                                        )
                                    },
                                )?
                            };
                        }
                        match operation {
                            ArrayCompareOp::Equal => mask_of!(|lhs, rhs| lhs == rhs),
                            ArrayCompareOp::NotEqual => mask_of!(|lhs, rhs| lhs != rhs),
                            ArrayCompareOp::Less => mask_of!(|lhs, rhs| lhs < rhs),
                            ArrayCompareOp::LessEqual => mask_of!(|lhs, rhs| lhs <= rhs),
                            ArrayCompareOp::Greater => mask_of!(|lhs, rhs| lhs > rhs),
                            ArrayCompareOp::GreaterEqual => mask_of!(|lhs, rhs| lhs >= rhs),
                        }
                    })*
                    ArrayStorage::Bool(_) => return Err(mask_error(name)),
                }
            };
        }
        let mask = per_dtype!(
            (Int32, array_int32_scalar),
            (Int64, array_int64_scalar),
            (Float32, array_float32_scalar),
            (Float64, array_float64_scalar)
        );
        Self::new(shape.into(), ArrayStorage::Bool(mask.into()))
    }

    /// `any`, `all`, or `count` of a mask.
    pub fn mask_reduce(&self, operation: ArrayMaskOp) -> Result<Value> {
        let elements = self.elements()?;
        let ArrayStorage::Bool(values) = &*elements else {
            return Err(dtype_error(operation.name(), "bool", self.dtype()));
        };
        Ok(match operation {
            ArrayMaskOp::Any => Value::Bool(values.contains(&true)),
            ArrayMaskOp::All => Value::Bool(!values.contains(&false)),
            ArrayMaskOp::Count => {
                // A block of at most 255 flags sums in one byte, which
                // the compiler adds many at a time.
                let count = values
                    .chunks(255)
                    .map(|block| {
                        usize::from(block.iter().map(|value| u8::from(*value)).sum::<u8>())
                    })
                    .sum::<usize>();
                Value::Int(IntegerValue::from_i64(count as i64))
            }
        })
    }

    /// An Array of `dtype` that takes each element from `when_true` where
    /// the mask `self` is `true` and from `when_false` elsewhere. Each
    /// argument is an Array of `dtype` that broadcasts with the mask, or
    /// one element.
    pub fn choose(&self, when_true: &Value, when_false: &Value, dtype: ArrayDType) -> Result<Self> {
        let ArrayStorage::Bool(mask) = &self.base else {
            return Err(dtype_error("choose", "bool", self.dtype()));
        };
        let mismatch = |array: &ArrayValue| {
            Diagnostic::coded(
                "AU4007",
                format!(
                    "array choose requires {} values whose shapes broadcast with the mask, found {:?} {} for a mask of {:?}",
                    dtype.runtime_type_name(),
                    array.shape,
                    array.dtype().runtime_type_name(),
                    self.shape
                ),
            )
        };
        let mut shape = self.shape.to_vec();
        for value in [when_true, when_false] {
            if let Value::Array(array) = value {
                if array.dtype() != dtype {
                    return Err(mismatch(array));
                }
                shape = array_layout::broadcast_shape(&shape, &array.shape)
                    .ok_or_else(|| mismatch(array))?;
            }
        }
        let mask_layout = array_layout::broadcast_layout(&self.shape, &self.layout(), &shape);
        let scalar_layout = repeated(&shape);
        let layout_of = |value: &Value| match value {
            Value::Array(array) => Cow::Owned(array_layout::broadcast_layout(
                &array.shape,
                &array.layout(),
                &shape,
            )),
            _ => Cow::Borrowed(&scalar_layout),
        };
        let (true_layout, false_layout) = (layout_of(when_true), layout_of(when_false));
        macro_rules! per_dtype {
            ($(($kind:ident, $variant:ident, $convert:ident)),*) => {
                match dtype {
                    $(ArrayDType::$kind => {
                        let (true_scalar, false_scalar);
                        let true_values = match when_true {
                            Value::Array(array) => {
                                let ArrayStorage::$variant(elements) = &array.base else {
                                    unreachable!("the dtype was checked above")
                                };
                                &**elements
                            }
                            scalar => {
                                true_scalar = [$convert(scalar, 0)?];
                                &true_scalar[..]
                            }
                        };
                        let false_values = match when_false {
                            Value::Array(array) => {
                                let ArrayStorage::$variant(elements) = &array.base else {
                                    unreachable!("the dtype was checked above")
                                };
                                &**elements
                            }
                            scalar => {
                                false_scalar = [$convert(scalar, 0)?];
                                &false_scalar[..]
                            }
                        };
                        let mut output =
                            try_array_buffer(shape.iter().product(), "array choose result")?;
                        array_layout::walk_runs(
                            &shape,
                            [&mask_layout, &*true_layout, &*false_layout],
                            |[mask_at, true_at, false_at], [mask_step, true_step, false_step], count| {
                                choose_run(
                                    &mut output,
                                    (mask, mask_at, mask_step),
                                    (true_values, true_at, true_step),
                                    (false_values, false_at, false_step),
                                    count,
                                );
                                true
                            },
                        );
                        ArrayStorage::$variant(output.into())
                    })*
                }
            };
        }
        let storage = per_dtype!(
            (Int32, Int32, array_int32_scalar),
            (Int64, Int64, array_int64_scalar),
            (Float32, Float32, array_float32_scalar),
            (Float64, Float64, array_float64_scalar),
            (Bool, Bool, array_bool_scalar)
        );
        Self::new(shape.into(), storage)
    }

    /// The Array with every element converted to `target` as the scalar
    /// cast converts it. A cast to the Array's own dtype shares its
    /// buffer.
    pub fn cast(&self, target: ArrayDType) -> Result<Self> {
        let source = self.dtype();
        if source == target {
            return Ok(self.clone());
        }
        if !source.is_numeric() || !target.is_numeric() {
            return Err(Diagnostic::coded(
                "AU4007",
                format!(
                    "array casts are only supported between numeric dtypes, found {} and {}",
                    source.runtime_type_name(),
                    target.runtime_type_name()
                ),
            ));
        }
        let label = "array cast result";
        let target_type = Type::named(target.runtime_type_name());
        // The scalar cast words the diagnostic of an element that fails.
        let refuse = |value: Value| {
            cast_numeric_value(value, &target_type, None)
                .err()
                .unwrap_or_else(|| Diagnostic::new("array cast failed"))
        };
        let refuse_int = |value: i64| refuse(Value::Int(IntegerValue::from_i64(value)));
        let refuse_float = |value: f64| refuse(Value::Float(value));
        const I64_LIMIT: f64 = 9_223_372_036_854_775_808.0;
        let float_to_i64 = |value: f64| {
            let truncated = value.trunc();
            if value.is_finite() && (-I64_LIMIT..I64_LIMIT).contains(&truncated) {
                Ok(truncated as i64)
            } else {
                Err(refuse_float(value))
            }
        };
        let float_to_i32 = |value: f64| {
            let truncated = value.trunc();
            if value.is_finite() && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&truncated)
            {
                Ok(truncated as i32)
            } else {
                Err(refuse_float(value))
            }
        };
        let i64_to_f64 = |value: i64| {
            let float = value as f64;
            if float as i128 == i128::from(value) {
                Ok(float)
            } else {
                Err(refuse_int(value))
            }
        };
        let i64_to_f32 = |value: i64| {
            let float = value as f32;
            if float as i128 == i128::from(value) {
                Ok(float)
            } else {
                Err(refuse_int(value))
            }
        };
        let elements = self.elements()?;
        let storage = match (&*elements, target) {
            (ArrayStorage::Int32(values), ArrayDType::Int64) => {
                ArrayStorage::Int64(map_elements(values, label, i64::from)?.into())
            }
            (ArrayStorage::Int32(values), ArrayDType::Float64) => {
                ArrayStorage::Float64(map_elements(values, label, f64::from)?.into())
            }
            (ArrayStorage::Int32(values), ArrayDType::Float32) => ArrayStorage::Float32(
                convert_elements(values, label, |value| i64_to_f32(i64::from(value)))?.into(),
            ),
            (ArrayStorage::Int64(values), ArrayDType::Int32) => ArrayStorage::Int32(
                convert_elements(values, label, |value| {
                    i32::try_from(value).map_err(|_| refuse_int(value))
                })?
                .into(),
            ),
            (ArrayStorage::Int64(values), ArrayDType::Float64) => {
                ArrayStorage::Float64(convert_elements(values, label, i64_to_f64)?.into())
            }
            (ArrayStorage::Int64(values), ArrayDType::Float32) => {
                ArrayStorage::Float32(convert_elements(values, label, i64_to_f32)?.into())
            }
            (ArrayStorage::Float32(values), ArrayDType::Int32) => ArrayStorage::Int32(
                convert_elements(values, label, |value| float_to_i32(f64::from(value)))?.into(),
            ),
            (ArrayStorage::Float32(values), ArrayDType::Int64) => ArrayStorage::Int64(
                convert_elements(values, label, |value| float_to_i64(f64::from(value)))?.into(),
            ),
            (ArrayStorage::Float32(values), ArrayDType::Float64) => {
                ArrayStorage::Float64(map_elements(values, label, f64::from)?.into())
            }
            (ArrayStorage::Float64(values), ArrayDType::Int32) => {
                ArrayStorage::Int32(convert_elements(values, label, float_to_i32)?.into())
            }
            (ArrayStorage::Float64(values), ArrayDType::Int64) => {
                ArrayStorage::Int64(convert_elements(values, label, float_to_i64)?.into())
            }
            (ArrayStorage::Float64(values), ArrayDType::Float32) => {
                ArrayStorage::Float32(map_elements(values, label, |value| value as f32)?.into())
            }
            _ => unreachable!("equal and mask dtypes were handled above"),
        };
        Self::new(self.try_shape()?, storage)
    }
}

#[cfg(test)]
#[path = "array_members_tests.rs"]
mod tests;
