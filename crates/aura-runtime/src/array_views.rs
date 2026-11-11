//! Array views and shape operations (ADR-0088): multi-axis selection,
//! `reshape`, `permute`, `transpose`, and assignment through a selection.
//!
//! A selection, a permutation, and a reshape of a contiguous Array share
//! the source's buffer and cost the same for any length. They are still
//! values: a later write to either Array copies first, so neither sees the
//! other change.

use crate::array_layout::{self, Strides};
use crate::diag::{Diagnostic, Result};
use crate::runtime_value::{
    array_float32_scalar, array_float64_scalar, array_int32_scalar, array_int64_scalar,
    normalize_array_coordinate, stepped_slice_positions, ArrayStorage, ArrayValue, Value,
};

/// What an index expression selects along one axis.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ArrayAxisSelection {
    /// One position. The axis is dropped from the result.
    Index(i64),
    /// `start:end:step`. The axis stays, with the selected length.
    Slice {
        start: Option<i64>,
        end: Option<i64>,
        step: i64,
    },
}

/// The words generated code passes per axis of a selection: a kind word,
/// then the start, end, and step.
pub const ARRAY_SELECTION_WORDS: usize = 4;
/// The kind word of an index, whose position is the start word.
pub const ARRAY_SELECTION_INDEX: i64 = 0;
/// The kind bit of a slice.
pub const ARRAY_SELECTION_SLICE: i64 = 1;
/// The kind bit of a slice whose start is written.
pub const ARRAY_SELECTION_HAS_START: i64 = 2;
/// The kind bit of a slice whose end is written.
pub const ARRAY_SELECTION_HAS_END: i64 = 4;

impl ArrayAxisSelection {
    /// The selection that `words` encodes, [`ARRAY_SELECTION_WORDS`] per
    /// axis.
    pub fn decode(words: &[i64]) -> Result<Vec<Self>> {
        let malformed = || Diagnostic::coded("AU4001", "malformed Array selection words");
        if words.is_empty() || words.len() % ARRAY_SELECTION_WORDS != 0 {
            return Err(malformed());
        }
        words
            .chunks_exact(ARRAY_SELECTION_WORDS)
            .map(|axis| {
                let (kind, start, end, step) = (axis[0], axis[1], axis[2], axis[3]);
                if kind == ARRAY_SELECTION_INDEX {
                    return Ok(Self::Index(start));
                }
                let known =
                    ARRAY_SELECTION_SLICE | ARRAY_SELECTION_HAS_START | ARRAY_SELECTION_HAS_END;
                if kind & ARRAY_SELECTION_SLICE == 0 || kind & !known != 0 {
                    return Err(malformed());
                }
                Ok(Self::Slice {
                    start: (kind & ARRAY_SELECTION_HAS_START != 0).then_some(start),
                    end: (kind & ARRAY_SELECTION_HAS_END != 0).then_some(end),
                    step,
                })
            })
            .collect()
    }
}

fn shape_error(message: String) -> Diagnostic {
    Diagnostic::coded("AU4007", message)
}

impl ArrayValue {
    /// The shape and layout that `selection` picks out of this Array, one
    /// selector per leading axis. Axes without a selector are kept whole.
    fn select_layout(&self, selection: &[ArrayAxisSelection]) -> Result<(Vec<usize>, Strides)> {
        let rank = self.rank();
        if selection.len() > rank {
            return Err(shape_error(format!(
                "array rank {rank} accepts at most {rank} selectors, found {}",
                selection.len()
            )));
        }
        let layout = self.layout();
        let mut offset = layout.offset as isize;
        let mut shape = Vec::with_capacity(rank);
        let mut strides = Vec::with_capacity(rank);
        for (axis, (&dimension, &stride)) in
            self.shape.iter().zip(layout.strides.iter()).enumerate()
        {
            match selection.get(axis) {
                None => {
                    shape.push(dimension);
                    strides.push(stride);
                }
                Some(ArrayAxisSelection::Index(coordinate)) => {
                    let position = normalize_array_coordinate(*coordinate, axis, dimension)?;
                    offset += position as isize * stride;
                }
                Some(ArrayAxisSelection::Slice { start, end, step }) => {
                    let (first, count) = stepped_slice_positions(
                        start.map(i128::from),
                        end.map(i128::from),
                        i128::from(*step),
                        dimension,
                    )?;
                    offset += first as isize * stride;
                    shape.push(count);
                    // Two selected positions are `step` apart and both
                    // inside the axis, so the product fits. A shorter axis
                    // never takes a step.
                    strides.push(if count > 1 {
                        stride * *step as isize
                    } else {
                        0
                    });
                }
            }
        }
        if shape.is_empty() {
            // Every axis took an index: the one element, as a rank-1 Array.
            shape.push(1);
            strides.push(1);
        }
        if shape.contains(&0) {
            offset = 0;
        }
        Ok((
            shape,
            Strides {
                offset: offset as usize,
                strides: strides.into(),
            },
        ))
    }

    /// `array[a, b:c, ::k]`: the elements `selection` picks, as a view that
    /// shares this Array's buffer.
    pub fn select(&self, selection: &[ArrayAxisSelection]) -> Result<Self> {
        let (shape, layout) = self.select_layout(selection)?;
        Ok(Self::view(&self.base, shape.into(), layout))
    }

    /// The Array's elements under a new shape of the same element count.
    /// One dimension may be `-1`, which takes whatever length the others
    /// leave. A contiguous Array shares its buffer with the result.
    pub fn reshape(&self, shape: &[i64]) -> Result<Self> {
        if shape.is_empty() {
            return Err(shape_error("array rank must be at least one".to_string()));
        }
        let len = self.len();
        let mut inferred = None;
        let mut known = Some(1_usize);
        for (axis, &dimension) in shape.iter().enumerate() {
            if dimension == -1 {
                if inferred.is_some() {
                    return Err(shape_error(format!(
                        "array reshape accepts one inferred dimension `-1`, found another at axis {axis}"
                    )));
                }
                inferred = Some(axis);
            } else if dimension < 0 {
                return Err(shape_error(format!(
                    "Array shape axis {axis} cannot be negative, found {dimension}"
                )));
            } else {
                known = known.and_then(|known| known.checked_mul(dimension as usize));
            }
        }
        let mismatch = || {
            shape_error(format!(
                "array reshape requires {len} elements, but shape {shape:?} does not hold that many"
            ))
        };
        let known = known.ok_or_else(mismatch)?;
        let mut dimensions: Vec<usize> = shape
            .iter()
            .map(|&dimension| dimension.max(0) as usize)
            .collect();
        match inferred {
            Some(axis) => {
                if known == 0 || len % known != 0 {
                    return Err(mismatch());
                }
                dimensions[axis] = len / known;
            }
            None if known != len => return Err(mismatch()),
            None => {}
        }
        Self::new(dimensions.into(), self.elements()?.into_owned())
    }

    /// The Array with its axes reordered: axis `i` of the result is axis
    /// `axes[i]` of this Array. The result is a view of the same buffer.
    pub fn permute(&self, axes: &[i64]) -> Result<Self> {
        let rank = self.rank();
        if axes.len() != rank {
            return Err(shape_error(format!(
                "array permute requires one axis per dimension of rank {rank}, found {}",
                axes.len()
            )));
        }
        let layout = self.layout();
        let mut seen = vec![false; rank];
        let mut shape = Vec::with_capacity(rank);
        let mut strides = Vec::with_capacity(rank);
        for &axis in axes {
            let normalized = self.normalize_axis(axis)?;
            if std::mem::replace(&mut seen[normalized], true) {
                return Err(shape_error(format!(
                    "array permute names axis {normalized} more than once"
                )));
            }
            shape.push(self.shape[normalized]);
            strides.push(layout.strides[normalized]);
        }
        Ok(Self::view(
            &self.base,
            shape.into(),
            Strides {
                offset: layout.offset,
                strides: strides.into(),
            },
        ))
    }

    /// The Array with its axes in reverse order, so a matrix's rows become
    /// its columns.
    pub fn transpose(&self) -> Result<Self> {
        let axes: Vec<i64> = (0..self.rank() as i64).rev().collect();
        self.permute(&axes)
    }

    /// The axis `axis` names. A negative axis counts from the last.
    pub(crate) fn normalize_axis(&self, axis: i64) -> Result<usize> {
        let rank = self.rank() as i64;
        let normalized = if axis < 0 { axis + rank } else { axis };
        if !(0..rank).contains(&normalized) {
            return Err(shape_error(format!(
                "array axis {axis} is outside rank {rank}"
            )));
        }
        Ok(normalized as usize)
    }

    /// `array[selection] = value`: writes a scalar, or an Array that
    /// broadcasts to the selected shape, over the selected elements.
    pub fn assign(&mut self, selection: &[ArrayAxisSelection], value: &Value) -> Result<()> {
        // Check the selection and the value's shape before anything is
        // copied.
        let (shape, _) = self.select_layout(selection)?;
        let source = match value {
            Value::Array(source) => {
                let fits = self.dtype() == source.dtype()
                    && array_layout::broadcast_shape(&shape, &source.shape).as_deref()
                        == Some(&shape[..]);
                if !fits {
                    return Err(shape_error(format!(
                        "array slice assignment requires a {} value that broadcasts to the selected shape {shape:?}, found {} {:?}",
                        self.dtype().runtime_type_name(),
                        source.dtype().runtime_type_name(),
                        source.shape
                    )));
                }
                Some((
                    source,
                    array_layout::broadcast_layout(&source.shape, &source.layout(), &shape),
                ))
            }
            _ => None,
        };
        // Writing gives this Array a row-major buffer of its own. A source
        // that is a view of this Array keeps the old buffer, so it reads
        // the elements as they were before the assignment.
        self.elements_mut()?;
        let (shape, target) = self.select_layout(selection)?;
        let label = "Array slice assignment";
        match (&mut self.base, source) {
            (ArrayStorage::Int32(values), Some((source, layout))) => {
                let ArrayStorage::Int32(source) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                let values = values.try_make_mut(label)?;
                array_layout::scatter(values, &target, source, &layout, &shape);
            }
            (ArrayStorage::Int64(values), Some((source, layout))) => {
                let ArrayStorage::Int64(source) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                let values = values.try_make_mut(label)?;
                array_layout::scatter(values, &target, source, &layout, &shape);
            }
            (ArrayStorage::Float32(values), Some((source, layout))) => {
                let ArrayStorage::Float32(source) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                let values = values.try_make_mut(label)?;
                array_layout::scatter(values, &target, source, &layout, &shape);
            }
            (ArrayStorage::Float64(values), Some((source, layout))) => {
                let ArrayStorage::Float64(source) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                let values = values.try_make_mut(label)?;
                array_layout::scatter(values, &target, source, &layout, &shape);
            }
            (ArrayStorage::Int32(values), None) => {
                let scalar = array_int32_scalar(value, 0)?;
                let values = values.try_make_mut(label)?;
                array_layout::scatter_fill(values, &target, &shape, scalar);
            }
            (ArrayStorage::Int64(values), None) => {
                let scalar = array_int64_scalar(value, 0)?;
                let values = values.try_make_mut(label)?;
                array_layout::scatter_fill(values, &target, &shape, scalar);
            }
            (ArrayStorage::Float32(values), None) => {
                let scalar = array_float32_scalar(value, 0)?;
                let values = values.try_make_mut(label)?;
                array_layout::scatter_fill(values, &target, &shape, scalar);
            }
            (ArrayStorage::Float64(values), None) => {
                let scalar = array_float64_scalar(value, 0)?;
                let values = values.try_make_mut(label)?;
                array_layout::scatter_fill(values, &target, &shape, scalar);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "array_views_tests.rs"]
mod tests;
