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
    pub(crate) fn new(shape: &[usize], axis: usize) -> Result<Self> {
        let inner = checked_array_element_count(&shape[axis + 1..])?;
        Ok(Self {
            inner,
            length: shape[axis],
        })
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
    let (count, _) = checked_tensor_byte_len(&shape, sources[0].dtype())?;
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
            let mut out = try_array_buffer(count, "tensor kernel result")?;
            for index in 0..count {
                out.push(map(index).map_or($zero, |(source, at)| slices[source][at]));
            }
            ArrayStorage::$variant(out.into())
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
    if step == 0 {
        return Err(tensor_error(
            "tensor::slice",
            "expects a step of at least 1",
        ));
    }
    let from = Lanes::new(&source.shape, axis)?;
    if length > 0 {
        let last = (length - 1)
            .checked_mul(step)
            .and_then(|offset| start.checked_add(offset))
            .filter(|&last| last < from.length);
        if last.is_none() {
            return Err(tensor_error(
                "tensor::slice",
                "slice bounds exceed the source axis length",
            ));
        }
    }
    let mut shape = source.shape.clone();
    shape[axis] = length;
    checked_tensor_byte_len(&shape, source.dtype())?;
    let to = Lanes::new(&shape, axis)?;
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
    if step == 0 {
        return Err(tensor_error(
            "tensor::unslice",
            "expects a step of at least 1",
        ));
    }
    let from = Lanes::new(&source.shape, axis)?;
    let mut shape = source.shape.clone();
    shape[axis] = size;
    checked_tensor_byte_len(&shape, source.dtype())?;
    let to = Lanes::new(&shape, axis)?;
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
        Lanes::new(&left.shape, axis)?,
        Lanes::new(&right.shape, axis)?,
    );
    let mut shape = left.shape.clone();
    shape[axis] = checked_concat_axis(shape[axis], right.shape[axis])?;
    checked_tensor_byte_len(&shape, left.dtype())?;
    let to = Lanes::new(&shape, axis)?;
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
        ArrayStorage::Int32(values) => {
            let mut out = try_array_buffer(values.len(), name)?;
            for &index in values.iter() {
                out.push(check(index.into())?);
            }
            Ok(out)
        }
        ArrayStorage::Int64(values) => {
            let mut out = try_array_buffer(values.len(), name)?;
            for &index in values.iter() {
                out.push(check(index)?);
            }
            Ok(out)
        }
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
    let from = Lanes::new(&source.shape, axis)?;
    let places = checked_indices(indices, from.length, "tensor::index_select")?;
    let shape = take_shape(&source.shape, &indices.shape, axis);
    checked_tensor_byte_len(&shape, source.dtype())?;
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
    let inner = checked_array_element_count(&updates.shape[axis + indices.shape.len()..])?;
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
    let (count, _) = checked_tensor_byte_len(&shape, updates.dtype())?;
    macro_rules! build {
        ($variant:ident, $values:expr, $zero:expr, $add:expr) => {{
            let mut sums = try_array_buffer(count, "tensor::index_add result")?;
            sums.resize(count, $zero);
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
    let lanes = Lanes::new(&source.shape, axis)?;
    let shape = lane_shape(&source.shape, axis);
    let (count, _) = checked_tensor_byte_len(&shape, ArrayDType::Int64)?;
    macro_rules! build {
        ($values:expr, $is_nan:expr) => {{
            let mut out = try_array_buffer(count, "tensor::argmax result")?;
            for lane in 0..count {
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
                out.push(place);
            }
            out
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

/// A `float64` in `[0, 1)` from the top 53 bits of `bits`.
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

/// A `float32` in `[0, 1)` from the top 24 bits of `bits`.
fn unit_single(bits: u64) -> f32 {
    (bits >> 40) as f32 / (1_u32 << 24) as f32
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

impl Fill {
    pub(crate) const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Uniform),
            1 => Some(Self::Normal),
            2 => Some(Self::Count),
            _ => None,
        }
    }
}

/// An Array of `shape` filled as `fill` says. Element `j` of a random
/// fill depends on `seed` and `j` only. A normal float is made from two
/// uniform ones by the method of Box and Muller, in the dtype's own
/// arithmetic.
pub(crate) fn filled(
    fill: Fill,
    shape: Box<[usize]>,
    seed: u64,
    dtype: ArrayDType,
) -> Result<ArrayValue> {
    let (count, _) = checked_tensor_byte_len(&shape, dtype)?;
    let storage = match (dtype, fill) {
        (ArrayDType::Float32, Fill::Count) => {
            let mut out = try_array_buffer(count, "tensor::filled result")?;
            for index in 0..count {
                out.push(index as f32);
            }
            ArrayStorage::Float32(out.into())
        }
        (ArrayDType::Float32, _) => {
            let mut out = try_array_buffer(count, "tensor::filled result")?;
            for index in 0..count as u64 {
                let value = if fill == Fill::Uniform {
                    unit_single(random_bits(seed, index))
                } else {
                    let first = unit_single(random_bits(seed, 2 * index));
                    let second = unit_single(random_bits(seed, 2 * index + 1));
                    (-2.0 * (1.0 - first).ln()).sqrt() * (std::f32::consts::TAU * second).cos()
                };
                out.push(value);
            }
            ArrayStorage::Float32(out.into())
        }
        (ArrayDType::Float64, Fill::Count) => {
            let mut out = try_array_buffer(count, "tensor::filled result")?;
            for index in 0..count {
                out.push(index as f64);
            }
            ArrayStorage::Float64(out.into())
        }
        (ArrayDType::Float64, _) => {
            let mut out = try_array_buffer(count, "tensor::filled result")?;
            for index in 0..count as u64 {
                let value = if fill == Fill::Uniform {
                    unit(random_bits(seed, index))
                } else {
                    let first = unit(random_bits(seed, 2 * index));
                    let second = unit(random_bits(seed, 2 * index + 1));
                    (-2.0 * (1.0 - first).ln()).sqrt() * (std::f64::consts::TAU * second).cos()
                };
                out.push(value);
            }
            ArrayStorage::Float64(out.into())
        }
        (ArrayDType::Int64, Fill::Count) => {
            let mut out = try_array_buffer(count, "tensor::filled result")?;
            for index in 0..count {
                out.push(index as i64);
            }
            ArrayStorage::Int64(out.into())
        }
        (ArrayDType::Int32, Fill::Count) => {
            let mut out = try_array_buffer(count, "tensor::filled result")?;
            for index in 0..count {
                out.push(index as i32);
            }
            ArrayStorage::Int32(out.into())
        }
        _ => {
            return Err(tensor_error(
                "tensor::random",
                "makes `float32` or `float64` elements",
            ))
        }
    };
    ArrayValue::new(shape, storage)
}

/// A half-precision float as a `float32`. Every half is one exactly.
pub(crate) fn half_to_single(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from(bits >> 10) & 0x1F;
    let fraction = u32::from(bits) & 0x3FF;
    let bits = match (exponent, fraction) {
        (0, 0) => sign,
        (0, _) => {
            // A subnormal half is a normal float32.
            let shift = fraction.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | ((fraction << shift) & 0x3FF) << 13
        }
        (0x1F, _) => sign | 0x7F80_0000 | (fraction << 13),
        _ => sign | ((exponent + 112) << 23) | (fraction << 13),
    };
    f32::from_bits(bits)
}

/// The half-precision float nearest to `value`, ties to even: an
/// infinity past the largest half, and a NaN for a NaN.
pub(crate) fn single_to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let fraction = bits & 0x007F_FFFF;
    if exponent == 0xFF {
        // An infinity, or a NaN, which stays one.
        let kept = if fraction == 0 {
            0
        } else {
            0x200 | (fraction >> 13) as u16
        };
        return sign | 0x7C00 | kept;
    }
    // The exponent of the half, before its bias of 15 is removed.
    let half_exponent = exponent - 112;
    if half_exponent >= 0x1F {
        return sign | 0x7C00;
    }
    if half_exponent <= 0 {
        // A subnormal half, or zero: the fraction with its leading one,
        // moved right until the exponent is the smallest.
        if half_exponent < -10 {
            return sign;
        }
        let whole = fraction | 0x0080_0000;
        let shift = (14 - half_exponent) as u32;
        let kept = whole >> shift;
        let rest = whole & ((1 << shift) - 1);
        let middle = 1 << (shift - 1);
        let up = rest > middle || (rest == middle && kept & 1 == 1);
        return sign | (kept as u16 + u16::from(up));
    }
    let kept = ((half_exponent as u32) << 10) | (fraction >> 13);
    let rest = fraction & 0x1FFF;
    let up = rest > 0x1000 || (rest == 0x1000 && kept & 1 == 1);
    // A carry out of the fraction raises the exponent, and out of the
    // largest exponent gives an infinity: both are the next bit pattern.
    sign | (kept as u16 + u16::from(up))
}

/// `value` as the nearest half-precision float.
pub(crate) fn rounded_to_half(value: f32) -> f32 {
    half_to_single(single_to_half(value))
}

/// A `float32` Array with every element rounded to half precision: the
/// host memory of a `float16` tensor.
pub(crate) fn round_half(elements: &ArrayValue) -> Result<ArrayValue> {
    let values = elements.elements()?;
    let ArrayStorage::Float32(values) = &*values else {
        return Err(Diagnostic::coded(
            "AU4007",
            "`tensor` keeps a float16 tensor as float32 elements".to_string(),
        ));
    };
    let rounded: Box<[f32]> = values.iter().map(|&value| rounded_to_half(value)).collect();
    ArrayValue::new(elements.try_shape()?, ArrayStorage::Float32(rounded.into()))
}

/// The half-precision bits of a `float32` Array, little-endian, as the
/// CUDA device and a safetensors file keep them.
pub(crate) fn half_bytes(elements: &ArrayValue) -> Result<Vec<u8>> {
    let values = elements.elements()?;
    let ArrayStorage::Float32(values) = &*values else {
        return Err(Diagnostic::coded(
            "AU4007",
            "`tensor` keeps a float16 tensor as float32 elements".to_string(),
        ));
    };
    Ok(values
        .iter()
        .flat_map(|&value| single_to_half(value).to_le_bytes())
        .collect())
}

/// A `float32` Array of `shape` from half-precision bits.
// Only the CUDA device reads halves back as bytes.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn from_half_bytes(shape: &[usize], bytes: &[u8]) -> Result<ArrayValue> {
    let values: Box<[f32]> = bytes
        .chunks_exact(2)
        .map(|chunk| half_to_single(u16::from_le_bytes([chunk[0], chunk[1]])))
        .collect();
    ArrayValue::new(shape.into(), ArrayStorage::Float32(values.into()))
}
