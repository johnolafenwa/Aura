//! The CPU kernels of the tensor operations that `Array` does not have:
//! the ones that move elements by index, the index of a maximum, the
//! error function, and random numbers. They define what each of these
//! operations means on every device.

use super::*;

/// How the linear index of an element splits around one axis: the
/// elements of a lane along the axis are `inner` apart.
#[derive(Clone, Copy)]
pub(crate) struct Lanes {
    /// The product of the dimensions after the axis.
    pub(crate) inner: usize,
    /// The length of the axis.
    pub(crate) length: usize,
}

impl Lanes {
    pub(crate) fn new(shape: &[usize], axis: usize) -> Self {
        Self {
            inner: shape[axis + 1..].iter().product(),
            length: shape[axis],
        }
    }

    /// The lane number before the axis, the place on the axis, and the
    /// place after it, of the element at linear index `index`.
    fn split(self, index: usize) -> (usize, usize, usize) {
        (
            index / (self.inner * self.length),
            (index / self.inner) % self.length,
            index % self.inner,
        )
    }

    /// The linear index of the element at `at` on the axis.
    fn join(self, outer: usize, at: usize, rest: usize) -> usize {
        (outer * self.length + at) * self.inner + rest
    }
}

/// An Array of `shape` whose element `j` is the element `map(j)` names,
/// as a source number and a linear index in it, or zero for `None`. The
/// sources have one dtype.
fn index_map(
    sources: &[&ArrayValue],
    shape: Box<[usize]>,
    map: impl Fn(usize) -> Option<(usize, usize)>,
) -> Result<ArrayValue> {
    let count: usize = shape.iter().product();
    let stores = sources
        .iter()
        .map(|source| source.elements())
        .collect::<Result<Vec<_>>>()?;
    macro_rules! build {
        ($variant:ident, $zero:expr) => {{
            let slices: Vec<&[_]> = stores
                .iter()
                .map(|store| match &**store {
                    ArrayStorage::$variant(values) => &values[..],
                    _ => unreachable!("the sources have one dtype"),
                })
                .collect();
            ArrayStorage::$variant(
                (0..count)
                    .map(|index| map(index).map_or($zero, |(source, at)| slices[source][at]))
                    .collect::<Box<[_]>>()
                    .into(),
            )
        }};
    }
    let storage = match &*stores[0] {
        ArrayStorage::Int32(_) => build!(Int32, 0),
        ArrayStorage::Int64(_) => build!(Int64, 0),
        ArrayStorage::Float32(_) => build!(Float32, 0.0),
        ArrayStorage::Float64(_) => build!(Float64, 0.0),
        ArrayStorage::Bool(_) => build!(Bool, false),
    };
    ArrayValue::new(shape, storage)
}

/// `length` elements of each lane along `axis`, from `start`, `step`
/// apart.
pub(crate) fn slice(
    source: &ArrayValue,
    axis: usize,
    start: usize,
    step: usize,
    length: usize,
) -> Result<ArrayValue> {
    let from = Lanes::new(&source.shape, axis);
    let mut shape = source.shape.clone();
    shape[axis] = length;
    let to = Lanes::new(&shape, axis);
    index_map(&[source], shape, |index| {
        let (outer, at, rest) = to.split(index);
        Some((0, from.join(outer, start + at * step, rest)))
    })
}

/// The reverse of [`slice`]: lanes of `size` elements along `axis` that
/// hold the elements of `source` from `start`, `step` apart, and zero
/// everywhere else.
pub(crate) fn unslice(
    source: &ArrayValue,
    axis: usize,
    start: usize,
    step: usize,
    size: usize,
) -> Result<ArrayValue> {
    let from = Lanes::new(&source.shape, axis);
    let mut shape = source.shape.clone();
    shape[axis] = size;
    let to = Lanes::new(&shape, axis);
    index_map(&[source], shape, |index| {
        let (outer, at, rest) = to.split(index);
        let offset = at.checked_sub(start)?;
        (offset.is_multiple_of(step) && offset / step < from.length)
            .then(|| (0, from.join(outer, offset / step, rest)))
    })
}

/// `source` repeated to `shape`, which its shape broadcasts to.
pub(crate) fn expand(source: &ArrayValue, shape: &[usize]) -> Result<ArrayValue> {
    index_map(&[source], shape.into(), |index| {
        Some((
            0,
            tensor_codegen::broadcast_offset(shape, &source.shape, index),
        ))
    })
}

/// The lanes of `left` along `axis`, each followed by the lane of
/// `right`.
pub(crate) fn concat(left: &ArrayValue, right: &ArrayValue, axis: usize) -> Result<ArrayValue> {
    let (first, second) = (
        Lanes::new(&left.shape, axis),
        Lanes::new(&right.shape, axis),
    );
    let mut shape = left.shape.clone();
    shape[axis] += right.shape[axis];
    let to = Lanes::new(&shape, axis);
    index_map(&[left, right], shape, |index| {
        let (outer, at, rest) = to.split(index);
        Some(if at < first.length {
            (0, first.join(outer, at, rest))
        } else {
            (1, second.join(outer, at - first.length, rest))
        })
    })
}

/// The indices of an integer Array, each checked against an axis of
/// `length` entries.
fn checked_indices(indices: &ArrayValue, length: usize, name: &str) -> Result<Vec<usize>> {
    let check = |index: i64| {
        usize::try_from(index)
            .ok()
            .filter(|&index| index < length)
            .ok_or_else(|| {
                tensor_error(
                    name,
                    format!("was given index {index} for an axis of length {length}"),
                )
            })
    };
    match &*indices.elements()? {
        ArrayStorage::Int32(values) => values.iter().map(|&index| check(index.into())).collect(),
        ArrayStorage::Int64(values) => values.iter().map(|&index| check(index)).collect(),
        _ => Err(tensor_error(name, "expects integer indices")),
    }
}

/// The shape of `take`: the source's with the axis replaced by the shape
/// of the indices.
pub(crate) fn take_shape(source: &[usize], indices: &[usize], axis: usize) -> Box<[usize]> {
    source[..axis]
        .iter()
        .chain(indices)
        .chain(&source[axis + 1..])
        .copied()
        .collect()
}

/// For each index, the entry of `source` at that place along `axis`.
pub(crate) fn take(source: &ArrayValue, indices: &ArrayValue, axis: usize) -> Result<ArrayValue> {
    let from = Lanes::new(&source.shape, axis);
    let places = checked_indices(indices, from.length, "tensor::take")?;
    let shape = take_shape(&source.shape, &indices.shape, axis);
    let to = Lanes {
        inner: from.inner,
        length: places.len(),
    };
    index_map(&[source], shape, |index| {
        let (outer, at, rest) = to.split(index);
        Some((0, from.join(outer, places[at], rest)))
    })
}

/// The reverse of [`take`]: lanes of `size` entries along `axis`, where
/// entry `k` is the sum of the entries of `updates` whose index is `k`.
pub(crate) fn index_add(
    updates: &ArrayValue,
    indices: &ArrayValue,
    axis: usize,
    size: usize,
) -> Result<ArrayValue> {
    let places = checked_indices(indices, size, "tensor::index_add")?;
    let inner: usize = updates.shape[axis + indices.shape.len()..].iter().product();
    let from = Lanes {
        inner,
        length: places.len(),
    };
    let to = Lanes {
        inner,
        length: size,
    };
    let shape: Box<[usize]> = updates.shape[..axis]
        .iter()
        .copied()
        .chain([size])
        .chain(updates.shape[axis + indices.shape.len()..].iter().copied())
        .collect();
    let count: usize = shape.iter().product();
    macro_rules! build {
        ($variant:ident, $values:expr, $zero:expr, $add:expr) => {{
            let mut sums = vec![$zero; count];
            for (index, &value) in $values.iter().enumerate() {
                let (outer, at, rest) = from.split(index);
                let sum = &mut sums[to.join(outer, places[at], rest)];
                *sum = $add(*sum, value);
            }
            ArrayStorage::$variant(sums.into())
        }};
    }
    let storage = match &*updates.elements()? {
        ArrayStorage::Int32(values) => build!(Int32, values, 0, i32::wrapping_add),
        ArrayStorage::Int64(values) => build!(Int64, values, 0, i64::wrapping_add),
        ArrayStorage::Float32(values) => build!(Float32, values, 0.0, |a: f32, b: f32| a + b),
        ArrayStorage::Float64(values) => build!(Float64, values, 0.0, |a: f64, b: f64| a + b),
        ArrayStorage::Bool(_) => {
            return Err(tensor_error(
                "tensor::index_add",
                "expects numbers, found a `Tensor[bool]`",
            ))
        }
    };
    ArrayValue::new(shape, storage)
}

/// The shape a reduction along `axis` gives: the axis is removed, and a
/// rank-one operand gives shape `[1]`.
pub(crate) fn lane_shape(shape: &[usize], axis: usize) -> Box<[usize]> {
    let mut shape = shape.to_vec();
    shape.remove(axis);
    if shape.is_empty() {
        shape.push(1);
    }
    shape.into()
}

/// The place along `axis` of the largest element of each lane, or of the
/// smallest. The first one wins a tie, and a NaN wins over every number.
pub(crate) fn arg_reduce(source: &ArrayValue, axis: usize, largest: bool) -> Result<ArrayValue> {
    let lanes = Lanes::new(&source.shape, axis);
    let shape = lane_shape(&source.shape, axis);
    let count: usize = shape.iter().product();
    macro_rules! build {
        ($values:expr, $is_nan:expr) => {{
            (0..count)
                .map(|lane| {
                    let (outer, rest) = (lane / lanes.inner, lane % lanes.inner);
                    let mut best = $values[lanes.join(outer, 0, rest)];
                    let mut place = 0_i64;
                    for at in 1..lanes.length {
                        let value = $values[lanes.join(outer, at, rest)];
                        let wins = if largest { value > best } else { value < best };
                        if wins || ($is_nan(value) && !$is_nan(best)) {
                            best = value;
                            place = at as i64;
                        }
                    }
                    place
                })
                .collect::<Box<[i64]>>()
        }};
    }
    let places = match &*source.elements()? {
        ArrayStorage::Int32(values) => build!(values, |_| false),
        ArrayStorage::Int64(values) => build!(values, |_| false),
        ArrayStorage::Float32(values) => build!(values, f32::is_nan),
        ArrayStorage::Float64(values) => build!(values, f64::is_nan),
        ArrayStorage::Bool(_) => {
            return Err(tensor_error(
                "tensor::argmax",
                "expects numbers, found a `Tensor[bool]`",
            ))
        }
    };
    ArrayValue::new(shape, ArrayStorage::Int64(places.into()))
}

/// The error function. Below 3 it is the series
/// `2/sqrt(pi) * exp(-x*x) * sum(2^n x^(2n+1) / (1*3*...*(2n+1)))`, whose
/// terms all have one sign. From 3 it is one minus the continued fraction
/// of the complementary function. The result is within a few units in
/// the last place of a `float64`.
pub(crate) fn erf(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let magnitude = x.abs();
    let scale = 2.0 / std::f64::consts::PI.sqrt();
    let result = if magnitude < 3.0 {
        let square = magnitude * magnitude;
        let mut term = magnitude;
        let mut sum = magnitude;
        let mut n = 0.0;
        while term > sum * 1e-17 {
            n += 1.0;
            term *= 2.0 * square / (2.0 * n + 1.0);
            sum += term;
        }
        scale * (-square).exp() * sum
    } else if magnitude < 6.0 {
        // erfc(x) = exp(-x*x) / sqrt(pi) / (x + (1/2)/(x + 1/(x + (3/2)/(x + ...))))
        let mut fraction = magnitude;
        for k in (1..=60).rev() {
            fraction = magnitude + (f64::from(k) / 2.0) / fraction;
        }
        1.0 - (-magnitude * magnitude).exp() / (std::f64::consts::PI.sqrt() * fraction)
    } else {
        1.0
    };
    result.copysign(x)
}

/// The logistic function, `1 / (1 + exp(-x))`.
pub(crate) fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// The 64 random bits for element `index` of the stream `seed` names.
/// Every element has its own bits, so a device makes them in any order.
pub(crate) fn random_bits(seed: u64, index: u64) -> u64 {
    let mut bits = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(index.wrapping_mul(0xD1B5_4A32_D192_ED03))
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    bits ^ (bits >> 31)
}

/// A float in `[0, 1)` from the top 53 bits of `bits`.
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

/// What [`filled`] fills a tensor with.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Fill {
    /// Floats spread evenly over `[0, 1)`.
    Uniform,
    /// Floats of the normal distribution with mean zero and deviation one.
    Normal,
    /// The integers from zero, in order.
    Count,
}

/// An Array of `shape` filled as `fill` says. Element `j` of a random
/// fill depends on `seed` and `j` only.
pub(crate) fn filled(
    fill: Fill,
    shape: Box<[usize]>,
    seed: u64,
    dtype: ArrayDType,
) -> Result<ArrayValue> {
    let count = checked_array_element_count(&shape)?;
    let value = |index: usize| -> f64 {
        let index = index as u64;
        match fill {
            Fill::Uniform => unit(random_bits(seed, index)),
            Fill::Normal => {
                // Box and Muller: two uniform floats give one normal one.
                let first = unit(random_bits(seed, 2 * index));
                let second = unit(random_bits(seed, 2 * index + 1));
                (-2.0 * (1.0 - first).ln()).sqrt() * (std::f64::consts::TAU * second).cos()
            }
            Fill::Count => index as f64,
        }
    };
    let storage = match dtype {
        ArrayDType::Float32 => ArrayStorage::Float32(
            (0..count)
                .map(|index| {
                    // A uniform float32 stays below one.
                    (value(index) as f32).min(if fill == Fill::Uniform {
                        1.0 - f32::EPSILON / 2.0
                    } else {
                        f32::INFINITY
                    })
                })
                .collect::<Box<[f32]>>()
                .into(),
        ),
        ArrayDType::Float64 => {
            ArrayStorage::Float64((0..count).map(value).collect::<Box<[f64]>>().into())
        }
        ArrayDType::Int64 if fill == Fill::Count => ArrayStorage::Int64(
            (0..count)
                .map(|index| index as i64)
                .collect::<Box<[i64]>>()
                .into(),
        ),
        ArrayDType::Int32 if fill == Fill::Count => ArrayStorage::Int32(
            (0..count)
                .map(|index| index as i32)
                .collect::<Box<[i32]>>()
                .into(),
        ),
        _ => {
            return Err(tensor_error(
                "tensor::random",
                "makes `float32` or `float64` elements",
            ))
        }
    };
    ArrayValue::new(shape, storage)
}
