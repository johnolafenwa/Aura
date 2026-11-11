//! Array views and shape operations (ADR-0088): multi-axis selection,
//! `reshape`, `permute`, `transpose`, and assignment through a selection.
//!
//! A selection, a permutation, and a reshape of a contiguous Array share
//! the source's buffer and cost the same for any length. They are still
//! values: a later write to either Array copies first, so neither sees the
//! other change.

use crate::array_layout::{self, AxisVec, Strides};
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

/// The kind word of a component that is not there. A selection of one
/// component that travels as [`ARRAY_SELECTION_INLINE_WORDS`] plain words
/// fills its second component with this.
pub const ARRAY_SELECTION_ABSENT: i64 = 8;
/// The words of a selection that travels without a tuple: two components.
pub const ARRAY_SELECTION_INLINE_WORDS: usize = 2 * ARRAY_SELECTION_WORDS;

impl ArrayAxisSelection {
    /// The selection that eight plain words encode: one or two components,
    /// the second possibly [`ARRAY_SELECTION_ABSENT`]. Returns the
    /// components and how many of them are present.
    #[inline]
    pub fn decode_inline(words: [i64; ARRAY_SELECTION_INLINE_WORDS]) -> Result<([Self; 2], usize)> {
        let first = Self::decode_axis(&words[..ARRAY_SELECTION_WORDS])?;
        if words[ARRAY_SELECTION_WORDS] == ARRAY_SELECTION_ABSENT {
            return Ok(([first, Self::Index(0)], 1));
        }
        let second = Self::decode_axis(&words[ARRAY_SELECTION_WORDS..])?;
        Ok(([first, second], 2))
    }

    #[inline]
    fn decode_axis(axis: &[i64]) -> Result<Self> {
        let (kind, start, end, step) = (axis[0], axis[1], axis[2], axis[3]);
        if kind == ARRAY_SELECTION_INDEX {
            return Ok(Self::Index(start));
        }
        let known = ARRAY_SELECTION_SLICE | ARRAY_SELECTION_HAS_START | ARRAY_SELECTION_HAS_END;
        if kind & ARRAY_SELECTION_SLICE == 0 || kind & !known != 0 {
            return Err(Diagnostic::coded(
                "AU4001",
                "malformed Array selection words",
            ));
        }
        Ok(Self::Slice {
            start: (kind & ARRAY_SELECTION_HAS_START != 0).then_some(start),
            end: (kind & ARRAY_SELECTION_HAS_END != 0).then_some(end),
            step,
        })
    }

    /// The selection that `words` encodes, [`ARRAY_SELECTION_WORDS`] per
    /// axis.
    pub fn decode(words: &[i64]) -> Result<Vec<Self>> {
        let malformed = || Diagnostic::coded("AU4001", "malformed Array selection words");
        if words.is_empty() || !words.len().is_multiple_of(ARRAY_SELECTION_WORDS) {
            return Err(malformed());
        }
        words
            .chunks_exact(ARRAY_SELECTION_WORDS)
            .map(Self::decode_axis)
            .collect()
    }
}

fn shape_error(message: String) -> Diagnostic {
    Diagnostic::coded("AU4007", message)
}

/// The layout of a selection, held without allocating for a short shape.
pub(crate) struct SelectedLayout {
    pub(crate) shape: AxisVec<usize>,
    pub(crate) offset: usize,
    pub(crate) strides: AxisVec<isize>,
}

impl SelectedLayout {
    /// Whether the selection is one row-major block.
    pub(crate) fn is_row_major(&self) -> bool {
        array_layout::is_row_major(&self.shape, &self.strides)
    }

    pub(crate) fn strides(&self) -> Strides {
        Strides {
            offset: self.offset,
            strides: self.strides.as_slice().into(),
        }
    }
}

impl ArrayValue {
    /// The shape, first position, and strides of the elements `selection`
    /// picks: one selector per leading axis. Axes without a selector are
    /// kept whole.
    pub(crate) fn select_parts(&self, selection: &[ArrayAxisSelection]) -> Result<SelectedLayout> {
        let rank = self.rank();
        if selection.len() > rank {
            return Err(shape_error(format!(
                "array rank {rank} accepts at most {rank} index components, found {}",
                selection.len()
            )));
        }
        let own_strides;
        let (mut offset, source_strides): (isize, &[isize]) = match &self.strided {
            Some(layout) => (layout.offset as isize, &layout.strides),
            None => {
                own_strides = array_layout::row_major_axis_strides(&self.shape);
                (0, &own_strides)
            }
        };
        let mut shape = AxisVec::new();
        let mut strides = AxisVec::new();
        for (axis, (&dimension, &stride)) in self.shape.iter().zip(source_strides).enumerate() {
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
                    let (first, count) = match (start, end, step) {
                        // `:` keeps the axis as it is.
                        (None, None, 1) => (0, dimension),
                        _ => stepped_slice_positions(
                            start.map(i128::from),
                            end.map(i128::from),
                            i128::from(*step),
                            dimension,
                        )?,
                    };
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
        Ok(SelectedLayout {
            shape,
            offset: offset as usize,
            strides,
        })
    }

    /// `array[a, b:c, ::k]`: the elements `selection` picks, as a view that
    /// shares this Array's buffer.
    pub fn select(&self, selection: &[ArrayAxisSelection]) -> Result<Self> {
        let selected = self.select_parts(selection)?;
        Ok(Self::view_of(
            &self.base,
            &selected.shape,
            selected.offset,
            &selected.strides,
        ))
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
                if known == 0 || !len.is_multiple_of(known) {
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

    /// `array[selection] = value` for a value the assignment owns. A value
    /// that is a view of this Array, as in `m[0, :] = m[1, :]`, gives up
    /// its view before the write, so the Array is written in place unless
    /// some other view of it is still alive.
    pub fn assign_owned(&mut self, selection: &[ArrayAxisSelection], value: Value) -> Result<()> {
        let value = match value {
            Value::Array(source) if source.shares_buffer_with(self) => {
                let selected = self.select_parts(selection)?;
                let source = match self.move_within(&selected, source) {
                    Ok(()) => return Ok(()),
                    Err(source) => source,
                };
                Value::Array(source.detached()?)
            }
            value => value,
        };
        self.assign(selection, &value)
    }

    /// Writes `source`, a block of this Array's own buffer, over the block
    /// `selected` names, inside the buffer. Hands `source` back when the
    /// write needs a copy of it: a strided side, a shape to broadcast, or
    /// a third reader of the buffer.
    fn move_within(
        &mut self,
        selected: &SelectedLayout,
        source: ArrayValue,
    ) -> std::result::Result<(), ArrayValue> {
        let blocks = self.strided.is_none()
            && source.strided.is_none()
            && *source.shape == *selected.shape
            && selected.is_row_major();
        if !blocks {
            return Err(source);
        }
        let ArrayValue { shape, base, .. } = source;
        macro_rules! move_block {
            ($($variant:ident),*) => {
                match (&mut self.base, base) {
                    $((ArrayStorage::$variant(values), ArrayStorage::$variant(block)) => {
                        if values.is_shared_only_with(&block) {
                            values.copy_within_from(block, selected.offset);
                            return Ok(());
                        }
                        ArrayStorage::$variant(block)
                    })*
                    (_, base) => base,
                }
            };
        }
        let base = move_block!(Int32, Int64, Float32, Float64);
        Err(ArrayValue {
            shape,
            base,
            strided: None,
        })
    }

    /// `array[selection] = value`: writes a scalar, or an Array that
    /// broadcasts to the selected shape, over the selected elements.
    pub fn assign(&mut self, selection: &[ArrayAxisSelection], value: &Value) -> Result<()> {
        // Check the selection and the value's shape before anything is
        // copied.
        let mut selected = self.select_parts(selection)?;
        let source = match value {
            Value::Array(source) => {
                let shape = selected.shape.as_slice();
                let fits = self.dtype() == source.dtype()
                    && (*source.shape == *shape
                        || array_layout::broadcast_shape(shape, &source.shape).as_deref()
                            == Some(shape));
                if !fits {
                    return Err(shape_error(format!(
                        "array slice assignment requires a value that broadcasts to the selected shape {shape:?} of {}, found {:?} {}",
                        self.dtype().runtime_type_name(),
                        source.shape,
                        source.dtype().runtime_type_name()
                    )));
                }
                Some(source)
            }
            _ => None,
        };
        // Writing gives this Array a row-major buffer of its own. A source
        // that is a view of this Array keeps the old buffer, so it reads
        // the elements as they were before the assignment.
        let repacked = self.strided.is_some();
        self.elements_mut()?;
        if repacked {
            selected = self.select_parts(selection)?;
        }
        let label = "Array slice assignment";
        match (&mut self.base, source) {
            (ArrayStorage::Int32(values), Some(source)) => {
                let ArrayStorage::Int32(elements) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                write_array(values.try_make_mut(label)?, &selected, elements, source);
            }
            (ArrayStorage::Int64(values), Some(source)) => {
                let ArrayStorage::Int64(elements) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                write_array(values.try_make_mut(label)?, &selected, elements, source);
            }
            (ArrayStorage::Float32(values), Some(source)) => {
                let ArrayStorage::Float32(elements) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                write_array(values.try_make_mut(label)?, &selected, elements, source);
            }
            (ArrayStorage::Float64(values), Some(source)) => {
                let ArrayStorage::Float64(elements) = &source.base else {
                    unreachable!("matching array dtypes imply matching storage variants")
                };
                write_array(values.try_make_mut(label)?, &selected, elements, source);
            }
            (ArrayStorage::Int32(values), None) => {
                let scalar = array_int32_scalar(value, 0)?;
                write_scalar(values.try_make_mut(label)?, &selected, scalar);
            }
            (ArrayStorage::Int64(values), None) => {
                let scalar = array_int64_scalar(value, 0)?;
                write_scalar(values.try_make_mut(label)?, &selected, scalar);
            }
            (ArrayStorage::Float32(values), None) => {
                let scalar = array_float32_scalar(value, 0)?;
                write_scalar(values.try_make_mut(label)?, &selected, scalar);
            }
            (ArrayStorage::Float64(values), None) => {
                let scalar = array_float64_scalar(value, 0)?;
                write_scalar(values.try_make_mut(label)?, &selected, scalar);
            }
        }
        Ok(())
    }
}

/// Writes `source`'s elements over the elements of `values` that `target`
/// selects. `source` broadcasts to the selected shape.
fn write_array<T: Copy>(
    values: &mut [T],
    target: &SelectedLayout,
    elements: &[T],
    source: &ArrayValue,
) {
    let shape = target.shape.as_slice();
    if source.strided.is_none() && *source.shape == *shape && target.is_row_major() {
        // One block over another of the same shape.
        values[target.offset..target.offset + elements.len()].copy_from_slice(elements);
        return;
    }
    let layout = array_layout::broadcast_layout(&source.shape, &source.layout(), shape);
    array_layout::scatter(values, &target.strides(), elements, &layout, shape);
}

/// Writes `scalar` over the elements of `values` that `target` selects.
fn write_scalar<T: Copy>(values: &mut [T], target: &SelectedLayout, scalar: T) {
    let shape = target.shape.as_slice();
    if target.is_row_major() {
        let len = shape.iter().product::<usize>();
        values[target.offset..target.offset + len].fill(scalar);
        return;
    }
    array_layout::scatter_fill(values, &target.strides(), shape, scalar);
}

#[cfg(test)]
#[path = "array_views_tests.rs"]
mod tests;
