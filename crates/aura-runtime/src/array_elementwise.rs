//! Elementwise Array arithmetic over operands that are not two blocks of
//! one shape (ADR-0089): a strided view, or a shape that broadcasts.
//!
//! The result is built one run at a time. A run is a stretch of the
//! result's row-major order along which each operand moves by one fixed
//! step. An operand that moves by one element lends its storage to the
//! kernel, one that stays on an element is a scalar, and any other is read
//! into a small scratch buffer first. Every run goes through the kernels
//! two blocks use, so the elements and the first reported fault are those
//! of the same operation on copied, row-major operands.

use crate::array_layout::{self, Strides};
use crate::array_simd;
use crate::array_views::ArrayAxisSelection;
use crate::diag::Result;
use crate::runtime_value::{
    array_float32_scalar, array_float64_scalar, array_int32_scalar, array_int64_scalar,
    array_storage_map, float_array_kernel_into, integer_array_kernel_into, try_array_buffer,
    ArrayBinaryOp, ArrayLanes, ArrayStorage, ArrayValue, IntegerArithmeticMode, Value,
};

/// An element type the binary kernels run on.
pub(crate) trait ArrayElement: Copy + Default {
    /// Appends `left op right` for `len` pairs to `output`. `base` is the
    /// flat index of the first pair.
    fn binary_into(
        output: &mut Vec<Self>,
        base: usize,
        len: usize,
        left: ArrayLanes<'_, Self>,
        right: ArrayLanes<'_, Self>,
        operation: ArrayBinaryOp,
        mode: IntegerArithmeticMode,
    ) -> Result<()>;
}

macro_rules! array_element {
    ($kernel:ident: $($element:ty),*) => {$(
        impl ArrayElement for $element {
            #[inline(always)]
            fn binary_into(
                output: &mut Vec<Self>,
                base: usize,
                len: usize,
                left: ArrayLanes<'_, Self>,
                right: ArrayLanes<'_, Self>,
                operation: ArrayBinaryOp,
                mode: IntegerArithmeticMode,
            ) -> Result<()> {
                $kernel(output, base, len, left, right, operation, mode)
            }
        }
    )*};
}

array_element!(integer_array_kernel_into: i32, i64);
array_element!(float_array_kernel_into: f32, f64);

/// The `count` elements of `base` from `position`, `step` apart, as a
/// kernel operand. A stride other than one or zero is read into
/// `scratch`.
#[inline(always)]
fn run_lanes<'a, T: Copy>(
    base: &'a [T],
    position: usize,
    step: isize,
    count: usize,
    scratch: &'a mut Vec<T>,
) -> ArrayLanes<'a, T> {
    match step {
        1 => ArrayLanes::Array(&base[position..position + count]),
        0 => ArrayLanes::Scalar(base[position]),
        _ => {
            let position = position as isize;
            scratch.clear();
            scratch
                .extend((0..count as isize).map(|index| base[(position + index * step) as usize]));
            ArrayLanes::Array(scratch)
        }
    }
}

/// `left op right` over two operands laid out for one result `shape`, in
/// row-major order.
#[inline(always)]
pub(crate) fn binary_elements<T: ArrayElement>(
    shape: &[usize],
    left: (&[T], &Strides),
    right: (&[T], &Strides),
    operation: ArrayBinaryOp,
    mode: IntegerArithmeticMode,
) -> Result<Vec<T>> {
    let len = shape.iter().product::<usize>();
    let mut output = try_array_buffer(len, "array arithmetic result")?;
    let (mut left_scratch, mut right_scratch) = (Vec::new(), Vec::new());
    let mut outcome = Ok(());
    array_layout::walk_runs(
        shape,
        [left.1, right.1],
        |[left_position, right_position], [left_step, right_step], count| {
            let left = run_lanes(left.0, left_position, left_step, count, &mut left_scratch);
            let right = run_lanes(
                right.0,
                right_position,
                right_step,
                count,
                &mut right_scratch,
            );
            let base = output.len();
            outcome = T::binary_into(&mut output, base, count, left, right, operation, mode);
            outcome.is_ok()
        },
    );
    outcome.map(|()| output)
}

/// `binary_elements` for two storages of one dtype.
#[inline(always)]
pub(crate) fn binary_storage(
    shape: &[usize],
    left: (&ArrayStorage, &Strides),
    right: (&ArrayStorage, &Strides),
    operation: ArrayBinaryOp,
    mode: IntegerArithmeticMode,
) -> Result<ArrayStorage> {
    macro_rules! per_dtype {
        ($($variant:ident),*) => {
            match (left.0, right.0) {
                $((ArrayStorage::$variant(left_values), ArrayStorage::$variant(right_values)) => {
                    ArrayStorage::$variant(
                        binary_elements(
                            shape,
                            (left_values, left.1),
                            (right_values, right.1),
                            operation,
                            mode,
                        )?
                        .into(),
                    )
                })*
                _ => unreachable!("matching array dtypes imply matching storage variants"),
            }
        };
    }
    Ok(per_dtype!(Int32, Int64, Float32, Float64))
}

/// `values op scalar`, or `scalar op values` when `scalar_left`, over a
/// strided view of `shape`.
#[inline(always)]
pub(crate) fn scalar_binary_storage(
    shape: &[usize],
    values: (&ArrayStorage, &Strides),
    scalar: &Value,
    scalar_left: bool,
    operation: ArrayBinaryOp,
    mode: IntegerArithmeticMode,
) -> Result<ArrayStorage> {
    // The scalar is one element that every axis repeats.
    let repeated = Strides {
        offset: 0,
        strides: vec![0; shape.len()].into(),
    };
    macro_rules! per_dtype {
        ($(($variant:ident, $convert:ident)),*) => {
            match values.0 {
                $(ArrayStorage::$variant(elements) => {
                    let scalar = [$convert(scalar, 0)?];
                    let (elements, scalar) = ((&**elements, values.1), (&scalar[..], &repeated));
                    let (left, right) = if scalar_left {
                        (scalar, elements)
                    } else {
                        (elements, scalar)
                    };
                    ArrayStorage::$variant(
                        binary_elements(shape, left, right, operation, mode)?.into(),
                    )
                })*
                ArrayStorage::Bool(_) => return Err(crate::runtime_value::mask_error("arithmetic")),
            }
        };
    }
    Ok(per_dtype!(
        (Int32, array_int32_scalar),
        (Int64, array_int64_scalar),
        (Float32, array_float32_scalar),
        (Float64, array_float64_scalar)
    ))
}

/// Elements per step of an in-place kernel: a result is computed into a
/// scratch block of this many elements and copied back, so it stays in
/// the first-level cache.
const IN_PLACE_BLOCK: usize = 1024;

/// The elements `start..end` of a kernel operand.
#[inline(always)]
fn lanes_between<T: Copy>(lanes: ArrayLanes<'_, T>, start: usize, end: usize) -> ArrayLanes<'_, T> {
    match lanes {
        ArrayLanes::Array(values) => ArrayLanes::Array(&values[start..end]),
        scalar @ ArrayLanes::Scalar(_) => scalar,
    }
}

/// `values = values op other`, or `other op values` when `values` is the
/// right operand, written over `values`. A fault is reported as the
/// kernels report it, with `base` the flat index of the first element,
/// and leaves the elements before its block updated.
#[inline(always)]
pub(crate) fn binary_in_place<T: ArrayElement>(
    values: &mut [T],
    base: usize,
    other: ArrayLanes<'_, T>,
    values_left: bool,
    operation: ArrayBinaryOp,
    mode: IntegerArithmeticMode,
    scratch: &mut Vec<T>,
) -> Result<()> {
    for start in (0..values.len()).step_by(IN_PLACE_BLOCK) {
        let end = (start + IN_PLACE_BLOCK).min(values.len());
        let own = ArrayLanes::Array(&values[start..end]);
        let other = lanes_between(other, start, end);
        let (left, right) = if values_left {
            (own, other)
        } else {
            (other, own)
        };
        scratch.clear();
        scratch.reserve(end - start);
        T::binary_into(
            scratch,
            base + start,
            end - start,
            left,
            right,
            operation,
            mode,
        )?;
        values[start..end].copy_from_slice(scratch);
    }
    Ok(())
}

/// Elements of a strided target updated between a read and its write.
const STRIDED_UPDATE_BLOCK: usize = 16;

/// `target op= other` over the elements of `values` that `target` selects,
/// with `other` laid out for the same `shape`. A fault leaves earlier runs
/// updated, so a caller that must not write on a fault rules it out first.
#[inline(always)]
pub(crate) fn update_elements<T: ArrayElement>(
    values: &mut [T],
    shape: &[usize],
    target: &Strides,
    other: (&[T], &Strides),
    operation: ArrayBinaryOp,
    mode: IntegerArithmeticMode,
) -> Result<()> {
    let (mut own_scratch, mut other_scratch, mut scratch) = (Vec::new(), Vec::new(), Vec::new());
    let mut outcome = Ok(());
    let mut base = 0;
    array_layout::walk_runs(
        shape,
        [target, other.1],
        |[position, other_position], [step, other_step], count| {
            let other = run_lanes(
                other.0,
                other_position,
                other_step,
                count,
                &mut other_scratch,
            );
            outcome = if step == 1 {
                let own = &mut values[position..position + count];
                binary_in_place(own, base, other, true, operation, mode, &mut scratch)
            } else {
                // A strided run is read out, computed, and written back a
                // few elements at a time, so each write lands on a cache
                // line its read just loaded.
                let position = position as isize;
                let mut computed = Ok(());
                for start in (0..count).step_by(STRIDED_UPDATE_BLOCK) {
                    let end = (start + STRIDED_UPDATE_BLOCK).min(count);
                    let slot = |index: usize| (position + index as isize * step) as usize;
                    own_scratch.clear();
                    own_scratch.extend((start..end).map(|index| values[slot(index)]));
                    computed = binary_in_place(
                        &mut own_scratch,
                        base + start,
                        lanes_between(other, start, end),
                        true,
                        operation,
                        mode,
                        &mut scratch,
                    );
                    if computed.is_err() {
                        break;
                    }
                    for (index, value) in (start..end).zip(&own_scratch) {
                        values[slot(index)] = *value;
                    }
                }
                computed
            };
            base += count;
            outcome.is_ok()
        },
    );
    outcome
}

/// Whether a divisor operand holds a zero anywhere in its storage.
fn has_zero<T: Copy + Default + PartialEq>(values: &[T]) -> bool {
    values.contains(&T::default())
}

impl ArrayValue {
    /// `self op other`, or `other op self` when `self` is the right
    /// operand, for a `self` nothing else can read: a temporary whose
    /// buffer no other Array shares. The result takes over `self`'s
    /// buffer, and `self` is left empty. Returns `None`, with `self`
    /// untouched, when the result does not fit that buffer.
    pub fn binary_reusing(
        &mut self,
        other: &Value,
        self_left: bool,
        operation: ArrayBinaryOp,
        mode: IntegerArithmeticMode,
    ) -> Option<Result<Self>> {
        if self.strided.is_some() || self.base.is_shared() {
            return None;
        }
        if let Value::Array(other) = other {
            let fits = other.strided.is_none()
                && other.shape == self.shape
                && other.dtype() == self.dtype();
            if !fits {
                return None;
            }
        }
        let label = "array arithmetic result";
        macro_rules! per_dtype {
            ($(($variant:ident, $convert:ident)),*) => {
                match (&mut self.base, other) {
                    $((ArrayStorage::$variant(values), Value::Array(other)) => {
                        let ArrayStorage::$variant(other) = &other.base else {
                            unreachable!("matching array dtypes imply matching storage variants")
                        };
                        let other = ArrayLanes::Array(&**other);
                        values.try_make_mut(label).and_then(|values| {
                            binary_in_place(values, 0, other, self_left, operation, mode, &mut Vec::new())
                        })
                    }
                    (ArrayStorage::$variant(values), scalar) => {
                        $convert(scalar, 0).and_then(|scalar| {
                            let other = ArrayLanes::Scalar(scalar);
                            let values = values.try_make_mut(label)?;
                            binary_in_place(values, 0, other, self_left, operation, mode, &mut Vec::new())
                        })
                    })*
                    (ArrayStorage::Bool(_), _) => Err(crate::runtime_value::mask_error("arithmetic")),
                }
            };
        }
        let outcome = array_simd::widest(
            #[inline(always)]
            || {
                per_dtype!(
                    (Int32, array_int32_scalar),
                    (Int64, array_int64_scalar),
                    (Float32, array_float32_scalar),
                    (Float64, array_float64_scalar)
                )
            },
        );
        Some(outcome.map(|()| {
            let empty = array_storage_map!(&self.base, |_values| Vec::new().into());
            Self {
                shape: std::mem::replace(&mut self.shape, Box::new([0])),
                base: std::mem::replace(&mut self.base, empty),
                strided: None,
            }
        }))
    }

    /// `array[selection] op= value`: the selected elements combined with
    /// a scalar, or with an Array that broadcasts to the selected shape.
    /// It is `array[selection] = array[selection] op value` with the
    /// selection worked out once. Float elements are updated where they
    /// lie. Nothing is written when the operation faults.
    pub fn update(
        &mut self,
        selection: &[ArrayAxisSelection],
        value: Value,
        operation: ArrayBinaryOp,
        mode: IntegerArithmeticMode,
    ) -> Result<()> {
        let value = match self.update_in_place(selection, value, operation, mode)? {
            None => return Ok(()),
            Some(value) => value,
        };
        // The general form: compute the new elements, then assign them.
        let view = self.select(selection)?;
        let computed = match &value {
            Value::Array(other) => view.binary(other, operation, mode),
            scalar => view.scalar_binary(scalar, false, operation, mode),
        }?;
        drop((view, value));
        self.assign_owned(selection, Value::Array(computed))
    }

    /// The in-place form of `update`, for float elements and a value that
    /// cannot fault. Hands the value back when the general form is needed.
    fn update_in_place(
        &mut self,
        selection: &[ArrayAxisSelection],
        value: Value,
        operation: ArrayBinaryOp,
        mode: IntegerArithmeticMode,
    ) -> Result<Option<Value>> {
        let float = matches!(
            self.base,
            ArrayStorage::Float32(_) | ArrayStorage::Float64(_)
        );
        if !float || mode != IntegerArithmeticMode::Checked {
            return Ok(Some(value));
        }
        let mut selected = self.select_parts(selection)?;
        // The update owns its value. One that reads this Array's buffer
        // is copied out, so this Array is the buffer's only reader.
        let value = match value {
            Value::Array(other) => {
                let fits = other.dtype() == self.dtype()
                    && array_layout::broadcast_shape(&selected.shape, &other.shape).as_deref()
                        == Some(selected.shape.as_slice());
                if !fits {
                    return Ok(Some(Value::Array(other)));
                }
                if other.shares_buffer_with(self) || other.strided.is_some() {
                    Value::Array(other.detached()?)
                } else {
                    Value::Array(other)
                }
            }
            scalar => scalar,
        };
        if operation == ArrayBinaryOp::Div {
            // A zero divisor is reported by the general form, which finds
            // its index and writes nothing.
            let zero = match &value {
                Value::Array(other) => match &other.base {
                    ArrayStorage::Float32(values) => has_zero(values),
                    ArrayStorage::Float64(values) => has_zero(values),
                    _ => true,
                },
                Value::Float(scalar) => *scalar == 0.0,
                _ => true,
            };
            if zero {
                return Ok(Some(value));
            }
        }
        let repacked = self.strided.is_some();
        self.elements_mut()?;
        if repacked {
            selected = self.select_parts(selection)?;
        }
        let shape = selected.shape.as_slice();
        let target = selected.strides();
        let label = "Array slice assignment";
        let repeated = Strides {
            offset: 0,
            strides: vec![0; shape.len()].into(),
        };
        macro_rules! per_dtype {
            ($(($variant:ident, $convert:ident)),*) => {
                match (&mut self.base, &value) {
                    $((ArrayStorage::$variant(values), Value::Array(other)) => {
                        let ArrayStorage::$variant(elements) = &other.base else {
                            unreachable!("matching array dtypes imply matching storage variants")
                        };
                        let layout = array_layout::broadcast_layout(&other.shape, &other.layout(), shape);
                        let values = values.try_make_mut(label)?;
                        update_elements(values, shape, &target, (elements, &layout), operation, mode)
                    }
                    (ArrayStorage::$variant(values), scalar) => {
                        let scalar = [$convert(scalar, 0)?];
                        let values = values.try_make_mut(label)?;
                        update_elements(values, shape, &target, (&scalar, &repeated), operation, mode)
                    })*
                    _ => unreachable!("only float Arrays update in place"),
                }
            };
        }
        array_simd::widest(
            #[inline(always)]
            || {
                per_dtype!(
                    (Float32, array_float32_scalar),
                    (Float64, array_float64_scalar)
                )
            },
        )?;
        Ok(None)
    }
}

#[cfg(test)]
#[path = "array_elementwise_tests.rs"]
mod tests;
