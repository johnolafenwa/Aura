//! Storage for the elements of a `list`.
//!
//! A list whose elements are all `int64`, all `float64`, or all `bool` keeps
//! the scalars themselves: 8 bytes for each `int64` or `float64` element and
//! 1 byte for each `bool`, instead of a whole `Value`. Any other list keeps
//! `Value`s. The storage follows the elements alone, and moving between the
//! two forms loses nothing, so a program cannot observe which one a list
//! uses. Generated code reads the scalar forms in place.
//!
//! Every form sits in a [`SharedVec`], so a clone shares the storage and the
//! first write through a shared handle copies it.

use std::borrow::Cow;
use std::collections::TryReserveError;
use std::fmt;

use crate::collection_index::SharedVec;
use crate::integer::{IntegerKind, IntegerValue};
use crate::randomness::DeterministicRng;
use crate::runtime_value::Value;

/// The elements of a `list`, in one of the forms the module docs describe.
#[derive(Clone)]
pub enum ListElements {
    Values(SharedVec<Value>),
    Int64(SharedVec<i64>),
    Float64(SharedVec<f64>),
    Bool(SharedVec<bool>),
}

/// Which form a [`ListElements`] uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListForm {
    Values,
    Int64,
    Float64,
    Bool,
}

/// A value that one of the scalar forms keeps exactly.
#[derive(Clone, Copy)]
enum Scalar {
    Int64(i64),
    Float64(f64),
    Bool(bool),
}

impl Scalar {
    fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Int(integer) => int64_bits(*integer).map(Self::Int64),
            Value::Float(float) => Some(Self::Float64(*float)),
            Value::Bool(flag) => Some(Self::Bool(*flag)),
            _ => None,
        }
    }
}

/// The bits of an `int64` value, when [`IntegerValue::from_i64`] rebuilds
/// exactly the same value from them. Integers of other kinds, and untyped
/// integer literals, stay boxed.
pub fn int64_bits(value: IntegerValue) -> Option<i64> {
    if value.runtime_kind() != Some(IntegerKind::Int64) {
        return None;
    }
    let bits = i64::try_from(value.as_i128()?).ok()?;
    (IntegerValue::from_i64(bits).representation() == value.representation()).then_some(bits)
}

#[inline]
fn unshared_capacity<T>(values: &SharedVec<T>) -> usize {
    if values.is_shared() {
        0
    } else {
        values.capacity()
    }
}

/// [`ListElements::adopt_appended`] for one form.
///
/// # Safety
///
/// As for [`ListElements::adopt_appended`].
unsafe fn adopt_appended<T>(values: &mut SharedVec<T>, len: usize) {
    if let Some(values) = values.unshared_mut() {
        if len > values.len() && len <= values.capacity() {
            // SAFETY: the caller wrote each element up to `len`.
            unsafe { values.set_len(len) };
        }
    }
}

fn int64_value(bits: i64) -> Value {
    Value::Int(IntegerValue::from_i64(bits))
}

impl ListElements {
    pub fn new() -> Self {
        Self::Values(SharedVec::new())
    }

    #[inline]
    pub fn form(&self) -> ListForm {
        match self {
            Self::Values(_) => ListForm::Values,
            Self::Int64(_) => ListForm::Int64,
            Self::Float64(_) => ListForm::Float64,
            Self::Bool(_) => ListForm::Bool,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Values(values) => values.len(),
            Self::Int64(values) => values.len(),
            Self::Float64(values) => values.len(),
            Self::Bool(values) => values.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The form, the address of the first element, the length, and how
    /// many elements a scalar form has room for when no other list shares
    /// its storage, or 0. Generated code reads the elements in place and
    /// appends while the room lasts.
    #[inline]
    pub fn view(&self) -> (ListForm, usize, usize, usize) {
        match self {
            Self::Values(values) => (ListForm::Values, values.as_ptr() as usize, values.len(), 0),
            Self::Int64(values) => (
                ListForm::Int64,
                values.as_ptr() as usize,
                values.len(),
                unshared_capacity(values),
            ),
            Self::Float64(values) => (
                ListForm::Float64,
                values.as_ptr() as usize,
                values.len(),
                unshared_capacity(values),
            ),
            Self::Bool(values) => (
                ListForm::Bool,
                values.as_ptr() as usize,
                values.len(),
                unshared_capacity(values),
            ),
        }
    }

    /// Takes in the elements generated code appended past the end of a
    /// scalar form, so the storage holds `len` elements. A length that is
    /// not past the end, or does not fit the storage, changes nothing.
    ///
    /// # Safety
    ///
    /// Every element from the current length up to `len` was written into
    /// this storage's spare room as a valid value of its form.
    pub unsafe fn adopt_appended(&mut self, len: usize) {
        match self {
            Self::Values(_) => {}
            Self::Int64(values) => unsafe { adopt_appended(values, len) },
            Self::Float64(values) => unsafe { adopt_appended(values, len) },
            Self::Bool(values) => unsafe { adopt_appended(values, len) },
        }
    }

    /// Whether another handle shares this storage.
    pub fn is_shared(&self) -> bool {
        match self {
            Self::Values(values) => values.is_shared(),
            Self::Int64(values) => values.is_shared(),
            Self::Float64(values) => values.is_shared(),
            Self::Bool(values) => values.is_shared(),
        }
    }

    /// The address of the first element. Two lists with the same address
    /// share their storage.
    #[inline]
    pub fn as_ptr(&self) -> usize {
        match self {
            Self::Values(values) => values.as_ptr() as usize,
            Self::Int64(values) => values.as_ptr() as usize,
            Self::Float64(values) => values.as_ptr() as usize,
            Self::Bool(values) => values.as_ptr() as usize,
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<Cow<'_, Value>> {
        match self {
            Self::Values(values) => values.get(index).map(Cow::Borrowed),
            Self::Int64(values) => values.get(index).map(|bits| Cow::Owned(int64_value(*bits))),
            Self::Float64(values) => values
                .get(index)
                .map(|float| Cow::Owned(Value::Float(*float))),
            Self::Bool(values) => values.get(index).map(|flag| Cow::Owned(Value::Bool(*flag))),
        }
    }

    /// The element at `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is out of range.
    pub fn at(&self, index: usize) -> Cow<'_, Value> {
        match self.get(index) {
            Some(element) => element,
            None => panic!(
                "list index {index} is out of range for length {}",
                self.len()
            ),
        }
    }

    pub fn first(&self) -> Option<Cow<'_, Value>> {
        self.get(0)
    }

    pub fn last(&self) -> Option<Cow<'_, Value>> {
        self.len().checked_sub(1).and_then(|index| self.get(index))
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        match self {
            Self::Values(values) => Iter::Values(values.iter()),
            Self::Int64(values) => Iter::Int64(values.iter()),
            Self::Float64(values) => Iter::Float64(values.iter()),
            Self::Bool(values) => Iter::Bool(values.iter()),
        }
    }

    /// The elements as a slice of `Value`s, when the list keeps `Value`s.
    #[inline]
    pub fn as_values(&self) -> Option<&[Value]> {
        match self {
            Self::Values(values) => Some(values),
            _ => None,
        }
    }

    /// The elements as a `Vec` of `Value`s to edit in place. A scalar list
    /// moves to the `Value` form first and keeps it, so this suits edits
    /// that reach inside an element. Edits of whole elements have their own
    /// methods, which keep a scalar form.
    pub fn values_mut(&mut self) -> &mut Vec<Value> {
        if !matches!(self, Self::Values(_)) {
            *self = Self::Values(self.to_vec().into());
        }
        match self {
            Self::Values(values) => values,
            _ => unreachable!("the list was moved to the value form"),
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Value> {
        if index >= self.len() {
            return None;
        }
        self.values_mut().get_mut(index)
    }

    pub fn last_mut(&mut self) -> Option<&mut Value> {
        if self.is_empty() {
            return None;
        }
        self.values_mut().last_mut()
    }

    /// Runs `edit` on the elements as `Value`s, then picks the form again.
    /// Use it for edits that cost a pass over the list anyway.
    pub fn update<R>(&mut self, edit: impl FnOnce(&mut Vec<Value>) -> R) -> R {
        let mut values = std::mem::take(self).into_vec();
        let result = edit(&mut values);
        *self = values.into();
        result
    }

    pub fn to_vec(&self) -> Vec<Value> {
        self.iter().map(Cow::into_owned).collect()
    }

    pub fn into_vec(self) -> Vec<Value> {
        match self {
            Self::Values(values) => values.into_vec(),
            other => other.to_vec(),
        }
    }

    #[inline]
    pub fn push(&mut self, value: Value) {
        match (&mut *self, Scalar::of(&value)) {
            (Self::Int64(values), Some(Scalar::Int64(bits))) => values.push(bits),
            (Self::Float64(values), Some(Scalar::Float64(float))) => values.push(float),
            (Self::Bool(values), Some(Scalar::Bool(flag))) => values.push(flag),
            (list, Some(scalar)) if list.is_empty() => *list = Self::single(scalar),
            (list, None) if list.is_empty() && !matches!(list, Self::Values(_)) => {
                *list = Self::Values(vec![value].into());
            }
            (list, _) => list.values_mut().push(value),
        }
    }

    fn single(scalar: Scalar) -> Self {
        match scalar {
            Scalar::Int64(bits) => Self::Int64(vec![bits].into()),
            Scalar::Float64(float) => Self::Float64(vec![float].into()),
            Scalar::Bool(flag) => Self::Bool(vec![flag].into()),
        }
    }

    pub fn pop(&mut self) -> Option<Value> {
        match self {
            Self::Values(values) => {
                if values.is_empty() {
                    None
                } else {
                    values.pop()
                }
            }
            Self::Int64(values) => {
                if values.is_empty() {
                    None
                } else {
                    values.pop().map(int64_value)
                }
            }
            Self::Float64(values) => {
                if values.is_empty() {
                    None
                } else {
                    values.pop().map(Value::Float)
                }
            }
            Self::Bool(values) => {
                if values.is_empty() {
                    None
                } else {
                    values.pop().map(Value::Bool)
                }
            }
        }
    }

    /// Replaces the element at `index` and returns the previous one.
    ///
    /// # Panics
    ///
    /// Panics when `index` is out of range.
    pub fn replace(&mut self, index: usize, value: Value) -> Value {
        assert!(
            index < self.len(),
            "list replace index {index} is out of range for length {}",
            self.len()
        );
        match (&mut *self, Scalar::of(&value)) {
            (Self::Int64(values), Some(Scalar::Int64(bits))) => {
                int64_value(std::mem::replace(&mut values[index], bits))
            }
            (Self::Float64(values), Some(Scalar::Float64(float))) => {
                Value::Float(std::mem::replace(&mut values[index], float))
            }
            (Self::Bool(values), Some(Scalar::Bool(flag))) => {
                Value::Bool(std::mem::replace(&mut values[index], flag))
            }
            (list, _) => std::mem::replace(&mut list.values_mut()[index], value),
        }
    }

    /// Inserts `value` before `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is past the end.
    pub fn insert(&mut self, index: usize, value: Value) {
        assert!(
            index <= self.len(),
            "list insert index {index} is out of range for length {}",
            self.len()
        );
        match (&mut *self, Scalar::of(&value)) {
            (Self::Int64(values), Some(Scalar::Int64(bits))) => values.insert(index, bits),
            (Self::Float64(values), Some(Scalar::Float64(float))) => values.insert(index, float),
            (Self::Bool(values), Some(Scalar::Bool(flag))) => values.insert(index, flag),
            (list, _) if list.is_empty() => list.push(value),
            (list, _) => list.values_mut().insert(index, value),
        }
    }

    /// Removes and returns the element at `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is out of range.
    pub fn remove(&mut self, index: usize) -> Value {
        assert!(
            index < self.len(),
            "list remove index {index} is out of range for length {}",
            self.len()
        );
        match self {
            Self::Values(values) => values.remove(index),
            Self::Int64(values) => int64_value(values.remove(index)),
            Self::Float64(values) => Value::Float(values.remove(index)),
            Self::Bool(values) => Value::Bool(values.remove(index)),
        }
    }

    pub fn swap(&mut self, first: usize, second: usize) {
        match self {
            Self::Values(values) => values.swap(first, second),
            Self::Int64(values) => values.swap(first, second),
            Self::Float64(values) => values.swap(first, second),
            Self::Bool(values) => values.swap(first, second),
        }
    }

    pub fn reverse(&mut self) {
        match self {
            Self::Values(values) => values.reverse(),
            Self::Int64(values) => values.reverse(),
            Self::Float64(values) => values.reverse(),
            Self::Bool(values) => values.reverse(),
        }
    }

    pub fn clear(&mut self) {
        // A fresh empty list drops shared storage without copying it.
        *self = Self::new();
    }

    pub fn truncate(&mut self, len: usize) {
        if len >= self.len() {
            return;
        }
        match self {
            Self::Values(values) => values.truncate(len),
            Self::Int64(values) => values.truncate(len),
            Self::Float64(values) => values.truncate(len),
            Self::Bool(values) => values.truncate(len),
        }
    }

    /// Appends every element of `other`.
    pub fn append(&mut self, other: ListElements) {
        if self.is_empty() {
            *self = other;
            return;
        }
        match (&mut *self, other) {
            (Self::Int64(values), Self::Int64(more)) => values.extend_from_slice(&more),
            (Self::Float64(values), Self::Float64(more)) => values.extend_from_slice(&more),
            (Self::Bool(values), Self::Bool(more)) => values.extend_from_slice(&more),
            (list, other) => {
                for value in other {
                    list.push(value);
                }
            }
        }
    }

    /// Removes the elements in `start..end` and returns them as a list.
    ///
    /// # Panics
    ///
    /// Panics when the range is out of order or past the end.
    pub fn take_range(&mut self, start: usize, end: usize) -> ListElements {
        assert!(
            start <= end && end <= self.len(),
            "list range {start}..{end} is out of range for length {}",
            self.len()
        );
        match self {
            Self::Values(values) => Self::from(values.drain(start..end).collect::<Vec<_>>()),
            Self::Int64(values) => Self::Int64(values.drain(start..end).collect()),
            Self::Float64(values) => Self::Float64(values.drain(start..end).collect()),
            Self::Bool(values) => Self::Bool(values.drain(start..end).collect()),
        }
    }

    /// Inserts every element of `other` before `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is past the end.
    pub fn insert_all(&mut self, index: usize, other: ListElements) {
        assert!(
            index <= self.len(),
            "list insert index {index} is out of range for length {}",
            self.len()
        );
        if other.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = other;
            return;
        }
        match (&mut *self, other) {
            (Self::Int64(values), Self::Int64(more)) => {
                values.splice(index..index, more.iter().copied());
            }
            (Self::Float64(values), Self::Float64(more)) => {
                values.splice(index..index, more.iter().copied());
            }
            (Self::Bool(values), Self::Bool(more)) => {
                values.splice(index..index, more.iter().copied());
            }
            (list, other) => {
                list.update(|values| {
                    values.splice(index..index, other);
                });
            }
        }
    }

    pub fn capacity(&self) -> usize {
        match self {
            Self::Values(values) => values.capacity(),
            Self::Int64(values) => values.capacity(),
            Self::Float64(values) => values.capacity(),
            Self::Bool(values) => values.capacity(),
        }
    }

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match self {
            Self::Values(values) => values.try_reserve(additional),
            Self::Int64(values) => values.try_reserve(additional),
            Self::Float64(values) => values.try_reserve(additional),
            Self::Bool(values) => values.try_reserve(additional),
        }
    }

    /// Shuffles the elements in place with `rng`.
    pub fn shuffle(&mut self, rng: &mut DeterministicRng) {
        match self {
            Self::Values(values) => rng.shuffle(values),
            Self::Int64(values) => rng.shuffle(values),
            Self::Float64(values) => rng.shuffle(values),
            Self::Bool(values) => rng.shuffle(values),
        }
    }

    pub fn contains(&self, needle: &Value) -> bool {
        match (self, needle) {
            (Self::Int64(values), Value::Int(integer)) => integer
                .as_i128()
                .and_then(|value| i64::try_from(value).ok())
                .is_some_and(|bits| values.contains(&bits)),
            (Self::Bool(values), Value::Bool(flag)) => values.contains(flag),
            _ => self.iter().any(|element| *element == *needle),
        }
    }
}

impl Default for ListElements {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Vec<Value>> for ListElements {
    /// Keeps the scalars when every element fits one scalar form.
    fn from(values: Vec<Value>) -> Self {
        match values.first().and_then(Scalar::of) {
            Some(Scalar::Int64(_)) => {
                let scalars = values
                    .iter()
                    .map(|value| match value {
                        Value::Int(integer) => int64_bits(*integer),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(scalars) = scalars {
                    return Self::Int64(scalars.into());
                }
            }
            Some(Scalar::Float64(_)) => {
                let scalars = values
                    .iter()
                    .map(|value| match value {
                        Value::Float(float) => Some(*float),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(scalars) = scalars {
                    return Self::Float64(scalars.into());
                }
            }
            Some(Scalar::Bool(_)) => {
                let scalars = values
                    .iter()
                    .map(|value| match value {
                        Value::Bool(flag) => Some(*flag),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(scalars) = scalars {
                    return Self::Bool(scalars.into());
                }
            }
            None => {}
        }
        Self::Values(values.into())
    }
}

impl From<SharedVec<Value>> for ListElements {
    fn from(values: SharedVec<Value>) -> Self {
        Self::from(values.into_vec())
    }
}

impl FromIterator<Value> for ListElements {
    fn from_iter<I: IntoIterator<Item = Value>>(values: I) -> Self {
        Self::from(values.into_iter().collect::<Vec<_>>())
    }
}

impl PartialEq for ListElements {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int64(left), Self::Int64(right)) => left[..] == right[..],
            (Self::Float64(left), Self::Float64(right)) => left[..] == right[..],
            (Self::Bool(left), Self::Bool(right)) => left[..] == right[..],
            // No pointer shortcut: a list holding NaN is not equal to itself.
            _ => {
                self.len() == other.len()
                    && self
                        .iter()
                        .zip(other.iter())
                        .all(|(left, right)| left == right)
            }
        }
    }
}

impl PartialEq<Vec<Value>> for ListElements {
    fn eq(&self, other: &Vec<Value>) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .zip(other.iter())
                .all(|(left, right)| *left == *right)
    }
}

impl fmt::Debug for ListElements {
    /// Matches the `SharedVec` the list held before the scalar forms, so
    /// debug output does not depend on the form.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Elements<'a>(&'a ListElements);
        impl fmt::Debug for Elements<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.debug_list().entries(self.0.iter()).finish()
            }
        }
        formatter
            .debug_tuple("SharedVec")
            .field(&Elements(self))
            .finish()
    }
}

/// The elements of a list in order, each borrowed from `Value` storage or
/// built from a scalar.
#[derive(Clone)]
pub enum Iter<'a> {
    Values(std::slice::Iter<'a, Value>),
    Int64(std::slice::Iter<'a, i64>),
    Float64(std::slice::Iter<'a, f64>),
    Bool(std::slice::Iter<'a, bool>),
}

impl<'a> Iterator for Iter<'a> {
    type Item = Cow<'a, Value>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Values(values) => values.next().map(Cow::Borrowed),
            Self::Int64(values) => values.next().map(|bits| Cow::Owned(int64_value(*bits))),
            Self::Float64(values) => values.next().map(|float| Cow::Owned(Value::Float(*float))),
            Self::Bool(values) => values.next().map(|flag| Cow::Owned(Value::Bool(*flag))),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Values(values) => values.next_back().map(Cow::Borrowed),
            Self::Int64(values) => values
                .next_back()
                .map(|bits| Cow::Owned(int64_value(*bits))),
            Self::Float64(values) => values
                .next_back()
                .map(|float| Cow::Owned(Value::Float(*float))),
            Self::Bool(values) => values
                .next_back()
                .map(|flag| Cow::Owned(Value::Bool(*flag))),
        }
    }
}

impl ExactSizeIterator for Iter<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Values(values) => values.len(),
            Self::Int64(values) => values.len(),
            Self::Float64(values) => values.len(),
            Self::Bool(values) => values.len(),
        }
    }
}

impl<'a> IntoIterator for &'a ListElements {
    type Item = Cow<'a, Value>;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for ListElements {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

#[cfg(test)]
#[path = "list_elements_tests.rs"]
mod tests;

/// Where one element of a `list[str]` keeps its UTF-8 bytes, as generated
/// code reads it from the list's `str` view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct StrViewEntry {
    /// The address of the first byte.
    pub ptr: usize,
    /// The length in bytes.
    pub len: usize,
}

/// A list's `str` view: one entry per element, and the element reads that
/// missed it since it was last built.
#[derive(Debug, Default)]
pub struct StrView {
    pub entries: Vec<StrViewEntry>,
    pub misses: usize,
}

/// The `str` view a list keeps once the native runtime builds it. It
/// describes the list's current elements only while the box's list view
/// says so, and a copy of the list starts without one.
#[derive(Default)]
pub struct StrViewCache(std::sync::atomic::AtomicPtr<StrView>);

impl StrViewCache {
    /// The view, made empty on first use.
    ///
    /// # Safety
    ///
    /// Only the thread the list's box is confined to may call it, and no
    /// other reference to the view may be alive.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get_or_create(&self) -> &mut StrView {
        use std::sync::atomic::Ordering;
        let mut view = self.0.load(Ordering::Relaxed);
        if view.is_null() {
            view = Box::into_raw(Box::default());
            self.0.store(view, Ordering::Relaxed);
        }
        // SAFETY: the box is this cache's, and the caller holds the only
        // reference.
        unsafe { &mut *view }
    }
}

impl Clone for StrViewCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Drop for StrViewCache {
    fn drop(&mut self) {
        let view = *self.0.get_mut();
        if !view.is_null() {
            // SAFETY: `get_or_create` leaked this box for the cache.
            drop(unsafe { Box::from_raw(view) });
        }
    }
}

impl fmt::Debug for StrViewCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StrViewCache")
    }
}
