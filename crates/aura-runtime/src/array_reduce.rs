//! Reductions along one axis of an Array (ADR-0089).
//!
//! Each element of the result is the reduction of one lane: the elements
//! that differ only in their coordinate along the reduced axis. The result
//! has the bits and the faults of the whole-Array reduction of that lane.
//!
//! A lane whose elements are neighbours in storage goes straight to the
//! whole-Array kernel. Otherwise an ordered `sum`, a `mean`, a `min`, or a
//! `max` is accumulated across the lanes: one pass over the source per
//! step along the axis, which adds each lane's elements in the same order
//! and reads storage in the order it lies. Any other lane is copied out
//! and given to the kernel.

use crate::array_layout::{self, Strides};
use crate::array_simd;
use crate::diag::{Diagnostic, Result};
use crate::runtime_value::{
    reduce_float32_array, reduce_float64_array, reduce_int32_array, reduce_int64_array,
    try_array_buffer, ArrayReduction, ArrayStorage, ArrayValue, Value,
};

/// An element type an axis reduction accumulates.
trait AxisElement: Copy + PartialOrd {
    const ZERO: Self;
    /// The whole-Array reduction of one lane.
    fn reduce(lane: &[Self], reduction: ArrayReduction) -> Result<Value>;
    /// The element a reduction result of this dtype holds.
    fn from_value(value: &Value) -> Self;
    /// `self + other`, or `None` when an integer sum leaves its type.
    fn add(self, other: Self) -> Option<Self>;
    fn to_f64(self) -> f64;
    /// Whether the accumulated value may differ from the kernel's: a
    /// float NaN, which the kernel makes canonical only for a NaN element.
    fn unsettled(self) -> bool;
    /// The lesser of two elements as `min` orders them, with `least`
    /// false for the greater as `max` orders them.
    fn extreme(self, other: Self, least: bool) -> Self;
}

macro_rules! integer_axis_element {
    ($(($element:ty, $kernel:ident)),*) => {$(
        impl AxisElement for $element {
            const ZERO: Self = 0;

            fn reduce(lane: &[Self], reduction: ArrayReduction) -> Result<Value> {
                $kernel(lane, reduction)
            }

            fn from_value(value: &Value) -> Self {
                match value {
                    Value::Int(integer) => integer
                        .as_i128()
                        .and_then(|value| <$element>::try_from(value).ok())
                        .expect("an integer reduction keeps its type"),
                    other => unreachable!("an integer reduction gave `{}`", other.render()),
                }
            }

            #[inline(always)]
            fn add(self, other: Self) -> Option<Self> {
                self.checked_add(other)
            }

            #[inline(always)]
            fn to_f64(self) -> f64 {
                self as f64
            }

            #[inline(always)]
            fn unsettled(self) -> bool {
                false
            }

            #[inline(always)]
            fn extreme(self, other: Self, least: bool) -> Self {
                if least {
                    self.min(other)
                } else {
                    self.max(other)
                }
            }
        }
    )*};
}

integer_axis_element!((i32, reduce_int32_array), (i64, reduce_int64_array));

macro_rules! float_axis_element {
    ($(($element:ty, $kernel:ident)),*) => {$(
        impl AxisElement for $element {
            const ZERO: Self = 0.0;

            fn reduce(lane: &[Self], reduction: ArrayReduction) -> Result<Value> {
                $kernel(lane, reduction)
            }

            fn from_value(value: &Value) -> Self {
                match value {
                    // A `float32` reduction widened its result exactly.
                    Value::Float(float) => *float as $element,
                    other => unreachable!("a float reduction gave `{}`", other.render()),
                }
            }

            #[inline(always)]
            fn add(self, other: Self) -> Option<Self> {
                Some(self + other)
            }

            #[inline(always)]
            fn to_f64(self) -> f64 {
                f64::from(self)
            }

            #[inline(always)]
            fn unsettled(self) -> bool {
                self.is_nan()
            }

            #[inline(always)]
            fn extreme(self, other: Self, least: bool) -> Self {
                if self.is_nan() || other.is_nan() {
                    return <$element>::NAN;
                }
                let other_wins = if least { other < self } else { other > self };
                if other_wins {
                    return other;
                }
                if self == other && self == 0.0 {
                    // Zeros compare equal: `min` keeps a negative zero and
                    // `max` a positive one.
                    let negative = self.is_sign_negative();
                    return if negative == least { self } else { other };
                }
                self
            }
        }
    )*};
}

float_axis_element!((f32, reduce_float32_array), (f64, reduce_float64_array));

/// Where the lanes of an axis reduction lie.
struct Lanes<'a> {
    /// The result's shape.
    shape: &'a [usize],
    /// Where each lane starts, laid out for the result's shape.
    starts: &'a Strides,
    /// The distance between a lane's neighbours.
    step: isize,
    /// Elements per lane.
    len: usize,
}

/// The elements of the lane that starts at `start`.
fn lane_elements<'a, T: Copy>(
    values: &'a [T],
    lanes: &Lanes<'_>,
    start: usize,
    scratch: &'a mut Vec<T>,
) -> &'a [T] {
    if lanes.step == 1 {
        return &values[start..start + lanes.len];
    }
    let start = start as isize;
    scratch.clear();
    scratch
        .extend((0..lanes.len as isize).map(|index| values[(start + index * lanes.step) as usize]));
    scratch
}

/// Calls `each` with the start of every lane, in the result's row-major
/// order, until it fails.
fn each_lane(lanes: &Lanes<'_>, mut each: impl FnMut(usize) -> Result<()>) -> Result<()> {
    let mut outcome = Ok(());
    array_layout::walk_runs(lanes.shape, [lanes.starts], |[position], [step], count| {
        let position = position as isize;
        for index in 0..count as isize {
            outcome = each((position + index * step) as usize);
            if outcome.is_err() {
                return false;
            }
        }
        true
    });
    outcome
}

/// Every lane through the whole-Array kernel, as elements of dtype `R`.
fn reduce_lanes<T: AxisElement, R>(
    values: &[T],
    lanes: &Lanes<'_>,
    reduction: ArrayReduction,
    convert: impl Fn(&Value) -> R,
) -> Result<Vec<R>> {
    let count = lanes.shape.iter().product::<usize>();
    let mut output = try_array_buffer(count, "array reduction result")?;
    let mut scratch = Vec::new();
    each_lane(lanes, |start| {
        let lane = lane_elements(values, lanes, start, &mut scratch);
        output.push(convert(&T::reduce(lane, reduction)?));
        Ok(())
    })?;
    Ok(output)
}

/// `accumulate(result, source)` for every result element and the element
/// of its lane at each step along the axis, in order. A lane's elements
/// reach `accumulate` in the order the kernel reads them.
#[inline(always)]
fn accumulate_lanes<T: Copy, A>(
    values: &[T],
    lanes: &Lanes<'_>,
    output: &mut [A],
    first: usize,
    mut accumulate: impl FnMut(&mut A, T),
) {
    let mut starts = lanes.starts.clone();
    for step in first..lanes.len {
        starts.offset = (lanes.starts.offset as isize + step as isize * lanes.step) as usize;
        let mut base = 0;
        array_layout::walk_runs(lanes.shape, [&starts], |[position], [stride], count| {
            let targets = &mut output[base..base + count];
            if stride == 1 {
                for (target, source) in targets.iter_mut().zip(&values[position..position + count])
                {
                    accumulate(target, *source);
                }
            } else {
                let position = position as isize;
                for (index, target) in targets.iter_mut().enumerate() {
                    accumulate(
                        target,
                        values[(position + index as isize * stride) as usize],
                    );
                }
            }
            base += count;
            true
        });
    }
}

/// The result of an axis reduction that keeps the dtype.
fn reduce_axis_same<T: AxisElement>(
    values: &[T],
    lanes: &Lanes<'_>,
    reduction: ArrayReduction,
) -> Result<Vec<T>> {
    let by_lane = || reduce_lanes(values, lanes, reduction, T::from_value);
    let accumulated = matches!(
        reduction,
        ArrayReduction::Sum | ArrayReduction::Min | ArrayReduction::Max
    ) && lanes.step != 1;
    if !accumulated {
        return by_lane();
    }
    let count = lanes.shape.iter().product::<usize>();
    let mut output: Vec<T> = try_array_buffer(count, "array reduction result")?;
    if reduction == ArrayReduction::Sum {
        output.resize(count, T::ZERO);
        let mut overflowed = false;
        array_simd::widest(
            #[inline(always)]
            || {
                accumulate_lanes(values, lanes, &mut output, 0, |sum, value| {
                    match sum.add(value) {
                        Some(next) => *sum = next,
                        None => overflowed = true,
                    }
                })
            },
        );
        if overflowed {
            // The kernel names the first lane and prefix that overflow.
            return by_lane();
        }
    } else {
        // `min` and `max` start from each lane's first element.
        let least = reduction == ArrayReduction::Min;
        array_layout::gather(values, lanes.shape, lanes.starts, &mut output);
        array_simd::widest(
            #[inline(always)]
            || {
                accumulate_lanes(values, lanes, &mut output, 1, |extreme, value| {
                    *extreme = extreme.extreme(value, least);
                })
            },
        );
    }
    if output.iter().any(|value| value.unsettled()) {
        // A NaN result takes the bits the kernel gives it.
        return by_lane();
    }
    Ok(output)
}

/// The result of an axis `mean` or `fast_mean`.
fn reduce_axis_mean<T: AxisElement>(
    values: &[T],
    lanes: &Lanes<'_>,
    reduction: ArrayReduction,
) -> Result<Vec<f64>> {
    let by_lane = || {
        reduce_lanes(values, lanes, reduction, |value| match value {
            Value::Float(mean) => *mean,
            other => unreachable!("a mean gave `{}`", other.render()),
        })
    };
    if reduction != ArrayReduction::Mean || lanes.step == 1 {
        return by_lane();
    }
    let count = lanes.shape.iter().product::<usize>();
    let mut output: Vec<f64> = try_array_buffer(count, "array reduction result")?;
    output.resize(count, 0.0);
    array_simd::widest(
        #[inline(always)]
        || {
            accumulate_lanes(values, lanes, &mut output, 0, |sum, value| {
                *sum += value.to_f64();
            })
        },
    );
    let len = lanes.len as f64;
    for mean in &mut output {
        *mean /= len;
    }
    if output.iter().any(|mean| mean.is_nan()) {
        return by_lane();
    }
    Ok(output)
}

impl ArrayValue {
    /// `array.sum(axis)` and the other reductions along one axis: an
    /// Array with `axis` removed whose elements reduce the lanes along
    /// it. A rank-1 Array gives one element.
    pub fn reduce_axis(&self, reduction: ArrayReduction, axis: i64) -> Result<Self> {
        if !self.dtype().is_numeric() {
            return Err(crate::runtime_value::mask_error(reduction.name()));
        }
        let axis = self.normalize_axis(axis)?;
        let layout = self.layout();
        let len = self.shape[axis];
        let mut shape: Vec<usize> = Vec::with_capacity(self.rank());
        let mut strides: Vec<isize> = Vec::with_capacity(self.rank());
        for (other, (&dimension, &stride)) in
            self.shape.iter().zip(layout.strides.iter()).enumerate()
        {
            if other != axis {
                shape.push(dimension);
                strides.push(stride);
            }
        }
        if shape.is_empty() {
            shape.push(1);
            strides.push(0);
        }
        let lanes = shape.iter().product::<usize>();
        let sums = matches!(reduction, ArrayReduction::Sum | ArrayReduction::FastSum);
        if len == 0 && lanes != 0 && !sums {
            return Err(Diagnostic::coded(
                "AU4007",
                format!(
                    "cannot compute array {} over an empty array",
                    reduction.name()
                ),
            ));
        }
        let starts = Strides {
            offset: layout.offset,
            strides: strides.into(),
        };
        let lanes = Lanes {
            shape: &shape,
            starts: &starts,
            step: layout.strides[axis],
            len,
        };
        let means = matches!(reduction, ArrayReduction::Mean | ArrayReduction::FastMean);
        let storage = match &self.base {
            base if len == 0 => {
                // Every lane is empty, and an empty sum is zero.
                let count = lanes.shape.iter().product::<usize>();
                match base {
                    ArrayStorage::Int32(_) => ArrayStorage::Int32(vec![0; count].into()),
                    ArrayStorage::Int64(_) => ArrayStorage::Int64(vec![0; count].into()),
                    ArrayStorage::Float32(_) => ArrayStorage::Float32(vec![0.0; count].into()),
                    ArrayStorage::Float64(_) => ArrayStorage::Float64(vec![0.0; count].into()),
                    ArrayStorage::Bool(_) => unreachable!("a mask was rejected above"),
                }
            }
            ArrayStorage::Int32(values) if means => {
                ArrayStorage::Float64(reduce_axis_mean(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Int64(values) if means => {
                ArrayStorage::Float64(reduce_axis_mean(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Float32(values) if means => {
                ArrayStorage::Float64(reduce_axis_mean(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Float64(values) if means => {
                ArrayStorage::Float64(reduce_axis_mean(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Int32(values) => {
                ArrayStorage::Int32(reduce_axis_same(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Int64(values) => {
                ArrayStorage::Int64(reduce_axis_same(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Float32(values) => {
                ArrayStorage::Float32(reduce_axis_same(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Float64(values) => {
                ArrayStorage::Float64(reduce_axis_same(values, &lanes, reduction)?.into())
            }
            ArrayStorage::Bool(_) => unreachable!("a mask was rejected above"),
        };
        Self::new(shape.into(), storage)
    }
}

#[cfg(test)]
#[path = "array_reduce_tests.rs"]
mod tests;
