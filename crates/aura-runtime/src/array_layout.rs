//! Shared Array storage and strided element layouts (ADR-0088).
//!
//! An Array's elements live in an [`ArrayBuf`]: a window of a buffer that
//! any number of Arrays share by reference count. Reading never copies. A
//! write goes through [`ArrayBuf::try_make_mut`], which first copies the
//! window when another Array still shares the buffer, so no Array ever
//! sees another's writes.
//!
//! A view that is not one row-major block, such as a transposed matrix or
//! a stepped slice, keeps a [`Strides`] record: the position of its first
//! element in the window and the distance between neighbours along each
//! axis. The walkers here visit such a layout in row-major order, in runs
//! along the innermost axes, so a kernel reads each run as a strided slice.

use std::fmt;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::diag::{Diagnostic, Result};
use crate::runtime_value::try_array_buffer;

/// A buffer and the number of windows that hold it. The count is the
/// buffer's reference count, kept where compiled code can read it: a
/// program writes an element in place only while the count is one
/// (ADR-0091). `repr(C)` puts it at the start of the block.
#[repr(C)]
struct ArrayOwner<T> {
    holders: AtomicUsize,
    elements: Box<[T]>,
}

/// A window of shared, immutable element storage.
pub struct ArrayBuf<T> {
    owner: Arc<ArrayOwner<T>>,
    start: usize,
    len: usize,
}

impl<T> Drop for ArrayBuf<T> {
    fn drop(&mut self) {
        self.owner.holders.fetch_sub(1, Ordering::Release);
    }
}

impl<T> ArrayBuf<T> {
    /// The elements `start..start + len` of this window, sharing its
    /// buffer.
    pub fn window(&self, start: usize, len: usize) -> Self {
        assert!(
            start <= self.len && len <= self.len - start,
            "an Array window stays inside its buffer"
        );
        self.owner.holders.fetch_add(1, Ordering::Relaxed);
        Self {
            owner: Arc::clone(&self.owner),
            start: self.start + start,
            len,
        }
    }

    /// How many windows hold the buffer.
    fn holders(&self) -> usize {
        self.owner.holders.load(Ordering::Acquire)
    }

    /// Whether another Array holds the same buffer.
    pub fn is_shared(&self) -> bool {
        self.holders() != 1
    }

    /// The address of the word that counts the buffer's holders. It is a
    /// `usize` that reads one while this window is the only holder, and it
    /// stays where it is for as long as the buffer lives.
    pub fn holders_address(&self) -> usize {
        std::ptr::from_ref(&self.owner.holders) as usize
    }

    /// Whether the window leaves out part of its buffer.
    pub fn is_partial(&self) -> bool {
        self.len != self.owner.elements.len()
    }

    /// Whether `self` and `other` are windows of one buffer.
    pub fn shares_buffer_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
    }
}

impl<T: Copy> ArrayBuf<T> {
    /// Whether this window and `other` are the only two holders of one
    /// buffer.
    pub fn is_shared_only_with(&self, other: &Self) -> bool {
        self.shares_buffer_with(other) && self.holders() == 2
    }

    /// Copies `source`, the one other window of this buffer, over the
    /// elements of this window from `at`, inside the buffer. The elements
    /// move as they were before the copy, even where the two ranges
    /// overlap. Returns `false`, having written nothing, when some third
    /// holder still reads the buffer.
    pub fn copy_within_from(&mut self, source: Self, at: usize) -> bool {
        if !self.is_shared_only_with(&source) {
            return false;
        }
        let (from, len) = (source.start, source.len);
        assert!(
            at <= self.len && len <= self.len - at,
            "an Array write stays inside its window"
        );
        drop(source);
        let to = self.start + at;
        let owner = Arc::get_mut(&mut self.owner).expect("the other holder was just dropped");
        owner.elements.copy_within(from..from + len, to);
        true
    }

    /// The window's elements for writing. A window whose buffer another
    /// Array shares is copied first, so the write stays private.
    pub fn try_make_mut(&mut self, label: &str) -> Result<&mut [T]> {
        if Arc::get_mut(&mut self.owner).is_none() {
            let mut copy = try_array_buffer(self.len, label)?;
            copy.extend_from_slice(self);
            *self = Self::from(copy);
        }
        let (start, len) = (self.start, self.len);
        let owner = Arc::get_mut(&mut self.owner).expect("the buffer was just made unique");
        Ok(&mut owner.elements[start..start + len])
    }
}

impl<T> Clone for ArrayBuf<T> {
    fn clone(&self) -> Self {
        self.window(0, self.len)
    }
}

impl<T> Deref for ArrayBuf<T> {
    type Target = [T];

    #[inline(always)]
    fn deref(&self) -> &[T] {
        &self.owner.elements[self.start..self.start + self.len]
    }
}

impl<T> From<Box<[T]>> for ArrayBuf<T> {
    fn from(values: Box<[T]>) -> Self {
        let len = values.len();
        Self {
            owner: Arc::new(ArrayOwner {
                holders: AtomicUsize::new(1),
                elements: values,
            }),
            start: 0,
            len,
        }
    }
}

impl<T> From<Vec<T>> for ArrayBuf<T> {
    fn from(values: Vec<T>) -> Self {
        Self::from(values.into_boxed_slice())
    }
}

impl<T, const N: usize> From<[T; N]> for ArrayBuf<T> {
    fn from(values: [T; N]) -> Self {
        Self::from(Vec::from(values))
    }
}

impl<T> FromIterator<T> for ArrayBuf<T> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        Self::from(values.into_iter().collect::<Vec<T>>())
    }
}

impl<T: PartialEq> PartialEq for ArrayBuf<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: fmt::Debug> fmt::Debug for ArrayBuf<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(formatter)
    }
}

/// Where a strided view's elements lie in its storage window: element
/// `[i0, i1, ...]` is at `offset + i0 * strides[0] + i1 * strides[1] ...`.
/// A stride is negative along an axis a slice walks backwards, and zero
/// along an axis that repeats one element.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Strides {
    pub offset: usize,
    pub strides: Box<[isize]>,
}

/// How many axes an [`AxisVec`] holds without allocating.
const INLINE_AXES: usize = 6;

/// One value per axis of an Array. Up to [`INLINE_AXES`] values live in
/// the value itself, so working out the layout of a view of a matrix
/// allocates nothing.
#[derive(Clone, Debug)]
pub struct AxisVec<T> {
    inline: [T; INLINE_AXES],
    len: usize,
    spilled: Vec<T>,
}

impl<T: Copy + Default> AxisVec<T> {
    pub fn new() -> Self {
        Self {
            inline: [T::default(); INLINE_AXES],
            len: 0,
            spilled: Vec::new(),
        }
    }

    pub fn push(&mut self, value: T) {
        if self.len < INLINE_AXES {
            self.inline[self.len] = value;
        } else {
            if self.len == INLINE_AXES {
                self.spilled.extend_from_slice(&self.inline);
            }
            self.spilled.push(value);
        }
        self.len += 1;
    }

    pub fn as_slice(&self) -> &[T] {
        if self.len <= INLINE_AXES {
            &self.inline[..self.len]
        } else {
            &self.spilled
        }
    }
}

impl<T: Copy + Default> Default for AxisVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default> Deref for AxisVec<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

/// The strides of a row-major block of `shape`, without allocating for a
/// short shape.
pub fn row_major_axis_strides(shape: &[usize]) -> AxisVec<isize> {
    let mut strides = AxisVec::new();
    if shape.len() > INLINE_AXES {
        for stride in row_major_strides(shape) {
            strides.push(stride);
        }
        return strides;
    }
    let mut block = 1_isize;
    for (stride, dimension) in strides.inline.iter_mut().zip(shape).rev() {
        *stride = block;
        block = block.wrapping_mul(*dimension as isize);
    }
    strides.len = shape.len();
    strides
}

/// The strides of a row-major block of `shape`.
pub fn row_major_strides(shape: &[usize]) -> Vec<isize> {
    let mut strides = vec![0_isize; shape.len()];
    let mut block = 1_isize;
    for (stride, dimension) in strides.iter_mut().zip(shape).rev() {
        *stride = block;
        // A live shape's element count fits `isize`, and an empty shape's
        // strides are never used.
        block = block.wrapping_mul(*dimension as isize);
    }
    strides
}

/// Whether a layout of `shape` with `strides` is one row-major block. An
/// axis of length one moves nothing, so its stride is free, and an empty
/// layout has no elements to misplace.
pub fn is_row_major(shape: &[usize], strides: &[isize]) -> bool {
    if shape.contains(&0) {
        return true;
    }
    let mut block = 1_isize;
    for (stride, dimension) in strides.iter().zip(shape).rev() {
        if *dimension != 1 && *stride != block {
            return false;
        }
        block *= *dimension as isize;
    }
    true
}

/// The number of trailing axes that all `N` operands lay out as one run:
/// the last axis, and each axis before it whose stride steps exactly over
/// the run that follows. Returns the first axis of the run and its length.
fn shared_run<const N: usize>(shape: &[usize], strides: &[&[isize]; N]) -> (usize, usize) {
    let last = shape.len() - 1;
    let mut first = last;
    let mut count = shape[last];
    while first > 0 {
        let outer = first - 1;
        let joins = strides
            .iter()
            .all(|strides| strides[outer] == strides[last] * count as isize);
        if !joins {
            break;
        }
        count *= shape[outer];
        first = outer;
    }
    (first, count)
}

/// Visits a layout of `shape` in row-major order, for `N` operands at
/// once. `run` receives each operand's position and stride at the start of
/// a run, and the run's length; it returns `false` to stop the walk.
/// Operands whose trailing axes are contiguous, or repeat one element, get
/// one long run instead of one per row.
pub fn walk_runs<const N: usize>(
    shape: &[usize],
    operands: [&Strides; N],
    mut run: impl FnMut([usize; N], [isize; N], usize) -> bool,
) {
    if shape.contains(&0) {
        return;
    }
    let strides = operands.map(|operand| &*operand.strides);
    let (run_axis, count) = shared_run(shape, &strides);
    let last = shape.len() - 1;
    let steps = strides.map(|strides| strides[last]);
    let mut positions = operands.map(|operand| operand.offset as isize);
    let mut coordinates = vec![0_usize; run_axis];
    loop {
        if !run(positions.map(|position| position as usize), steps, count) {
            return;
        }
        // Advance the odometer over the axes before the run.
        let mut axis = run_axis;
        loop {
            if axis == 0 {
                return;
            }
            axis -= 1;
            coordinates[axis] += 1;
            for (position, strides) in positions.iter_mut().zip(&strides) {
                *position += strides[axis];
            }
            if coordinates[axis] < shape[axis] {
                break;
            }
            for (position, strides) in positions.iter_mut().zip(&strides) {
                *position -= strides[axis] * shape[axis] as isize;
            }
            coordinates[axis] = 0;
        }
    }
}

/// Appends the elements of a strided layout to `output` in row-major
/// order.
pub fn gather<T: Copy>(base: &[T], shape: &[usize], layout: &Strides, output: &mut Vec<T>) {
    walk_runs(shape, [layout], |[start], [step], count| {
        if step == 1 {
            output.extend_from_slice(&base[start..start + count]);
        } else {
            // A counted iterator reserves once, where a push per element
            // checks the capacity each time.
            let start = start as isize;
            output.extend((0..count as isize).map(|index| base[(start + index * step) as usize]));
        }
        true
    });
}

/// Writes `source`, laid out by `source_layout`, over the elements of
/// `target` that `target_layout` selects. Both layouts have `shape`.
pub fn scatter<T: Copy>(
    target: &mut [T],
    target_layout: &Strides,
    source: &[T],
    source_layout: &Strides,
    shape: &[usize],
) {
    walk_runs(
        shape,
        [target_layout, source_layout],
        |[to, from], [to_step, from_step], count| {
            if to_step == 1 && from_step == 1 {
                target[to..to + count].copy_from_slice(&source[from..from + count]);
            } else if to_step == 1 && from_step == 0 {
                target[to..to + count].fill(source[from]);
            } else {
                let (mut to, mut from) = (to as isize, from as isize);
                for _ in 0..count {
                    target[to as usize] = source[from as usize];
                    to += to_step;
                    from += from_step;
                }
            }
            true
        },
    );
}

/// Writes `value` over the elements of `target` that `layout` selects.
pub fn scatter_fill<T: Copy>(target: &mut [T], layout: &Strides, shape: &[usize], value: T) {
    walk_runs(shape, [layout], |[to], [step], count| {
        if step == 1 {
            target[to..to + count].fill(value);
        } else {
            let mut to = to as isize;
            for _ in 0..count {
                target[to as usize] = value;
                to += step;
            }
        }
        true
    });
}

/// The shape two operand shapes broadcast to, or `None` when they do not
/// agree. Shapes align at their last axes. Two dimensions agree when they
/// are equal or one of them is one, and the result takes the other.
pub fn broadcast_shape(left: &[usize], right: &[usize]) -> Option<Vec<usize>> {
    let rank = left.len().max(right.len());
    let mut shape = vec![0_usize; rank];
    for (back, slot) in shape.iter_mut().rev().enumerate() {
        let dimension = |shape: &[usize]| {
            shape
                .len()
                .checked_sub(back + 1)
                .map_or(1, |axis| shape[axis])
        };
        let (left, right) = (dimension(left), dimension(right));
        *slot = match (left, right) {
            (left, right) if left == right => left,
            (1, other) | (other, 1) => other,
            _ => return None,
        };
    }
    Some(shape)
}

/// The layout that reads an operand of `shape` and `layout` as the
/// broadcast shape `target`: a missing leading axis and a stretched axis
/// of length one both repeat their element with a zero stride.
pub fn broadcast_layout(shape: &[usize], layout: &Strides, target: &[usize]) -> Strides {
    let lead = target.len() - shape.len();
    let strides = (0..target.len())
        .map(|axis| match axis.checked_sub(lead) {
            Some(own) if shape[own] == target[axis] => layout.strides[own],
            _ => 0,
        })
        .collect();
    Strides {
        offset: layout.offset,
        strides,
    }
}

/// The `AU4007` diagnostic for two shapes that do not broadcast.
pub fn broadcast_error(what: &str, left: &[usize], right: &[usize]) -> Diagnostic {
    Diagnostic::coded(
        "AU4007",
        format!("{what} requires shapes that broadcast together, found {left:?} and {right:?}"),
    )
}

#[cfg(test)]
#[path = "array_layout_tests.rs"]
mod tests;
