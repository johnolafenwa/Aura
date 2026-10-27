//! Storage for the elements of a `list`.
//!
//! A list whose elements are all `int64`, all `float64`, or all `bool` keeps
//! the scalars themselves: 8 bytes for each `int64` or `float64` element and
//! 1 byte for each `bool`, instead of a whole `Value`. A list whose elements
//! are all values of a class or tuple type that generated code registered a
//! plain layout for keeps each value's bytes in that layout, one after
//! another (see [`PlainLayout`]). Any other list keeps `Value`s. The storage
//! follows the elements alone, and moving between the forms loses nothing,
//! so a program cannot observe which one a list uses. Generated code reads
//! the scalar and typed forms in place.
//!
//! Every form sits in a [`SharedVec`], so a clone shares the storage and the
//! first write through a shared handle copies it.

use std::borrow::Cow;
use std::collections::TryReserveError;
use std::fmt;

use crate::collection_index::SharedVec;
use crate::integer::{IntegerKind, IntegerValue};
use crate::native_runtime::{plain_layout_for_value, PlainLayout};
use crate::randomness::DeterministicRng;
use crate::runtime_value::Value;

/// The elements of a `list`, in one of the forms the module docs describe.
#[derive(Clone)]
pub enum ListElements {
    Values(SharedVec<Value>),
    Int64(SharedVec<i64>),
    Float64(SharedVec<f64>),
    Bool(SharedVec<bool>),
    /// Boxed, so the typed form keeps every list one pointer and a tag.
    Typed(Box<TypedElements>),
}

/// Which form a [`ListElements`] uses. A typed list names its layout by
/// the layout's access code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListForm {
    Values,
    Int64,
    Float64,
    Bool,
    Typed(u32),
}

/// Values of one plain layout, in place: each element takes the layout's
/// stride of words, and the storage holds whole elements only.
#[derive(Clone)]
pub struct TypedElements {
    layout: &'static PlainLayout,
    words: SharedVec<u64>,
}

impl TypedElements {
    fn new(layout: &'static PlainLayout) -> Self {
        Self {
            layout,
            words: SharedVec::new(),
        }
    }

    /// The elements of `values` in `layout`, when every one fits it.
    fn of(layout: &'static PlainLayout, values: &[Value]) -> Option<Self> {
        let mut typed = Self::new(layout);
        for value in values {
            if !layout.fits(value) {
                return None;
            }
            typed.push(value);
        }
        Some(typed)
    }

    pub fn layout(&self) -> &'static PlainLayout {
        self.layout
    }

    fn stride(&self) -> usize {
        self.layout.stride_words()
    }

    fn len(&self) -> usize {
        self.words.len() / self.layout.stride()
    }

    /// [`ListElements::adopt_appended`] for this form, out of line for the
    /// same reason as [`Self::view`].
    ///
    /// # Safety
    ///
    /// As for [`ListElements::adopt_appended`].
    #[inline(never)]
    unsafe fn adopt_appended(&mut self, len: usize) {
        if let Some(words) = len.checked_mul(self.stride()) {
            unsafe { adopt_appended(&mut self.words, words) }
        }
    }

    /// [`ListElements::view`] for this form. It divides by the stride, so
    /// it stays out of line and the scalar forms' view stays small enough
    /// to inline into every list write.
    #[inline(never)]
    fn view(&self) -> (ListForm, usize, usize, usize) {
        (
            ListForm::Typed(self.layout.access()),
            self.words.as_ptr() as usize,
            self.len(),
            unshared_capacity(&self.words) / self.layout.stride(),
        )
    }

    fn range(&self, index: usize) -> std::ops::Range<usize> {
        index * self.stride()..(index + 1) * self.stride()
    }

    /// The words of element `index`, which hold its layout's bytes.
    pub fn element_words(&self, index: usize) -> &[u64] {
        &self.words[self.range(index)]
    }

    fn decode(&self, index: usize) -> Value {
        decode_typed(self.layout, &self.words[self.range(index)])
    }

    /// Encodes a value that fits the layout into `stride` new words.
    fn encoded(&self, value: &Value) -> Vec<u64> {
        let mut words = vec![0; self.stride()];
        // SAFETY: the caller checked that `value` fits, and `words` has
        // room for one element.
        unsafe { self.layout.encode(value, words.as_mut_ptr()) };
        words
    }

    fn push(&mut self, value: &Value) {
        let encoded = self.encoded(value);
        self.words.extend_from_slice(&encoded);
    }

    /// Appends the element whose words are at `src`.
    ///
    /// # Safety
    ///
    /// `src` is aligned for `u64` and holds a stride of initialized words
    /// that encode a value of this layout.
    unsafe fn push_words(&mut self, src: *const u64) {
        // SAFETY: the caller's contract.
        let words = unsafe { std::slice::from_raw_parts(src, self.stride()) };
        self.words.extend_from_slice(words);
    }

    /// Overwrites element `index` with the words at `src`.
    ///
    /// # Safety
    ///
    /// As for [`Self::push_words`].
    unsafe fn set_words(&mut self, index: usize, src: *const u64) {
        // SAFETY: the caller's contract.
        let words = unsafe { std::slice::from_raw_parts(src, self.stride()) };
        let range = self.range(index);
        self.words[range].copy_from_slice(words);
    }

    fn replace(&mut self, index: usize, value: &Value) -> Value {
        let previous = self.decode(index);
        let encoded = self.encoded(value);
        let range = self.range(index);
        self.words[range].copy_from_slice(&encoded);
        previous
    }

    fn insert(&mut self, index: usize, value: &Value) {
        let encoded = self.encoded(value);
        let at = index * self.stride();
        self.words.splice(at..at, encoded);
    }

    fn remove(&mut self, index: usize) -> Value {
        let value = self.decode(index);
        let range = self.range(index);
        self.words.drain(range);
        value
    }

    fn pop(&mut self) -> Option<Value> {
        let last = self.len().checked_sub(1)?;
        let value = self.decode(last);
        self.truncate(last);
        Some(value)
    }

    fn swap(&mut self, first: usize, second: usize) {
        if first == second {
            return;
        }
        let stride = self.stride();
        for word in 0..stride {
            self.words
                .swap(first * stride + word, second * stride + word);
        }
    }

    /// The elements in the order `order` lists their old positions.
    fn permuted(&self, order: impl Iterator<Item = usize>) -> Self {
        let mut words = Vec::with_capacity(self.words.len());
        for index in order {
            words.extend_from_slice(&self.words[self.range(index)]);
        }
        Self {
            layout: self.layout,
            words: words.into(),
        }
    }

    fn truncate(&mut self, len: usize) {
        let words = len * self.stride();
        if words < self.words.len() {
            self.words.truncate(words);
        }
    }
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
unsafe fn adopt_appended<T: Copy>(values: &mut SharedVec<T>, len: usize) {
    if let Some(values) = values.unshared_mut() {
        if len > values.len() && len <= values.capacity() {
            // SAFETY: the caller wrote each element up to `len`.
            unsafe { values.set_len(len) };
        } else if len < values.len() {
            // Generated code popped the elements past `len`.
            values.truncate(len);
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

    /// An empty list that keeps values of `layout` in place.
    pub fn typed(layout: &'static PlainLayout) -> Self {
        Self::Typed(Box::new(TypedElements::new(layout)))
    }

    #[inline]
    pub fn form(&self) -> ListForm {
        match self {
            Self::Values(_) => ListForm::Values,
            Self::Int64(_) => ListForm::Int64,
            Self::Float64(_) => ListForm::Float64,
            Self::Bool(_) => ListForm::Bool,
            Self::Typed(typed) => ListForm::Typed(typed.layout.access()),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        // Testing for the typed form first leaves a match whose arms all
        // read a `Vec`'s length the same way, which compiles to one load.
        if let Self::Typed(typed) = self {
            return typed.len();
        }
        match self {
            Self::Values(values) => values.len(),
            Self::Int64(values) => values.len(),
            Self::Float64(values) => values.len(),
            Self::Bool(values) => values.len(),
            Self::Typed(typed) => typed.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The form, the address of the first element, the length, and how
    /// many elements a scalar or typed form has room for when no other list
    /// shares its storage, or 0. Generated code reads the elements in place
    /// and appends while the room lasts.
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
            Self::Typed(typed) => typed.view(),
        }
    }

    /// Takes in the elements generated code appended past the end of a
    /// scalar or typed form, or drops the ones it popped, so the storage
    /// holds `len` elements. A length that does not fit the storage changes
    /// nothing.
    ///
    /// # Safety
    ///
    /// Every element from the current length up to `len` was written into
    /// this storage's spare room as a valid value of its form.
    #[inline]
    pub unsafe fn adopt_appended(&mut self, len: usize) {
        match self {
            Self::Values(_) => {}
            Self::Int64(values) => unsafe { adopt_appended(values, len) },
            Self::Float64(values) => unsafe { adopt_appended(values, len) },
            Self::Bool(values) => unsafe { adopt_appended(values, len) },
            Self::Typed(typed) => unsafe { typed.adopt_appended(len) },
        }
    }

    /// Whether another handle shares this storage.
    pub fn is_shared(&self) -> bool {
        match self {
            Self::Values(values) => values.is_shared(),
            Self::Int64(values) => values.is_shared(),
            Self::Float64(values) => values.is_shared(),
            Self::Bool(values) => values.is_shared(),
            Self::Typed(typed) => typed.words.is_shared(),
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
            Self::Typed(typed) => typed.words.as_ptr() as usize,
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
            Self::Typed(typed) => (index < typed.len()).then(|| Cow::Owned(typed.decode(index))),
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
            Self::Typed(typed) => Iter::Typed {
                layout: typed.layout,
                words: &typed.words,
            },
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
            (Self::Typed(typed), None) if typed.layout.fits(&value) => typed.push(&value),
            (list, Some(scalar)) if list.is_empty() => *list = Self::single(scalar),
            (list, None) if list.is_empty() => match plain_layout_for_value(&value) {
                Some(layout) => {
                    let mut typed = TypedElements::new(layout);
                    typed.push(&value);
                    *list = Self::Typed(Box::new(typed));
                }
                // An empty list of values keeps its storage and its room.
                None => list.values_mut().push(value),
            },
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

    /// Appends the plain record whose words are at `src`, as [`Self::push`]
    /// appends the value they hold. A list that keeps values of `layout`
    /// copies the words without building the value.
    ///
    /// # Safety
    ///
    /// `src` is aligned for `u64` and holds `layout.stride_words()`
    /// initialized words that encode a value of `layout`.
    pub unsafe fn push_plain(&mut self, layout: &'static PlainLayout, src: *const u64) {
        match self {
            Self::Typed(typed) if std::ptr::eq(typed.layout, layout) => {
                // SAFETY: the caller's contract.
                unsafe { typed.push_words(src) }
            }
            // SAFETY: the caller's contract.
            list => list.push(unsafe { layout.read(src.cast()) }),
        }
    }

    /// Replaces element `index`, which is in range, with the plain record
    /// whose words are at `src`, as [`Self::replace`] does with the value
    /// they hold.
    ///
    /// # Safety
    ///
    /// As for [`Self::push_plain`].
    pub unsafe fn replace_plain(
        &mut self,
        index: usize,
        layout: &'static PlainLayout,
        src: *const u64,
    ) {
        match self {
            Self::Typed(typed) if std::ptr::eq(typed.layout, layout) => {
                // SAFETY: the caller's contract, and `index` is in range.
                unsafe { typed.set_words(index, src) }
            }
            list => {
                // SAFETY: the caller's contract.
                list.replace(index, unsafe { layout.read(src.cast()) });
            }
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
            Self::Typed(typed) => typed.pop(),
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
            (Self::Typed(typed), None) if typed.layout.fits(&value) => typed.replace(index, &value),
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
            (Self::Typed(typed), None) if typed.layout.fits(&value) => typed.insert(index, &value),
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
            Self::Typed(typed) => typed.remove(index),
        }
    }

    pub fn swap(&mut self, first: usize, second: usize) {
        match self {
            Self::Values(values) => values.swap(first, second),
            Self::Int64(values) => values.swap(first, second),
            Self::Float64(values) => values.swap(first, second),
            Self::Bool(values) => values.swap(first, second),
            Self::Typed(typed) => {
                assert!(
                    first.max(second) < typed.len(),
                    "list swap index is out of range for length {}",
                    typed.len()
                );
                typed.swap(first, second)
            }
        }
    }

    pub fn reverse(&mut self) {
        match self {
            Self::Values(values) => values.reverse(),
            Self::Int64(values) => values.reverse(),
            Self::Float64(values) => values.reverse(),
            Self::Bool(values) => values.reverse(),
            Self::Typed(typed) => **typed = typed.permuted((0..typed.len()).rev()),
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
            Self::Typed(typed) => typed.truncate(len),
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
            (Self::Typed(typed), Self::Typed(more)) if std::ptr::eq(typed.layout, more.layout) => {
                typed.words.extend_from_slice(&more.words)
            }
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
            Self::Typed(typed) => {
                let stride = typed.stride();
                Self::Typed(Box::new(TypedElements {
                    layout: typed.layout,
                    words: typed.words.drain(start * stride..end * stride).collect(),
                }))
            }
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
            (Self::Typed(typed), Self::Typed(more)) if std::ptr::eq(typed.layout, more.layout) => {
                let at = index * typed.stride();
                typed.words.splice(at..at, more.words.iter().copied());
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
            Self::Typed(typed) => typed.words.capacity() / typed.layout.stride(),
        }
    }

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match self {
            Self::Values(values) => values.try_reserve(additional),
            Self::Int64(values) => values.try_reserve(additional),
            Self::Float64(values) => values.try_reserve(additional),
            Self::Bool(values) => values.try_reserve(additional),
            Self::Typed(typed) => {
                let stride = typed.stride();
                typed.words.try_reserve(additional.saturating_mul(stride))
            }
        }
    }

    /// Shuffles the elements in place with `rng`.
    pub fn shuffle(&mut self, rng: &mut DeterministicRng) {
        match self {
            Self::Values(values) => rng.shuffle(values),
            Self::Int64(values) => rng.shuffle(values),
            Self::Float64(values) => rng.shuffle(values),
            Self::Bool(values) => rng.shuffle(values),
            Self::Typed(typed) => {
                // The same draws as shuffling the elements themselves.
                let mut order = (0..typed.len()).collect::<Vec<_>>();
                rng.shuffle(&mut order);
                **typed = typed.permuted(order.into_iter());
            }
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
            None => {
                let typed = values
                    .first()
                    .and_then(plain_layout_for_value)
                    .and_then(|layout| TypedElements::of(layout, &values));
                if let Some(typed) = typed {
                    return Self::Typed(Box::new(typed));
                }
            }
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
    /// The words of the elements left, whole elements only.
    Typed {
        layout: &'static PlainLayout,
        words: &'a [u64],
    },
}

/// The value one typed element's words hold. Decoding builds a `Value`, so
/// it stays out of line and the other forms' steps stay small.
#[inline(never)]
fn decode_typed(layout: &PlainLayout, words: &[u64]) -> Value {
    // SAFETY: a typed list's words hold whole elements its layout wrote.
    unsafe { layout.decode(words.as_ptr()) }
}

impl<'a> Iterator for Iter<'a> {
    type Item = Cow<'a, Value>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Values(values) => values.next().map(Cow::Borrowed),
            Self::Int64(values) => values.next().map(|bits| Cow::Owned(int64_value(*bits))),
            Self::Float64(values) => values.next().map(|float| Cow::Owned(Value::Float(*float))),
            Self::Bool(values) => values.next().map(|flag| Cow::Owned(Value::Bool(*flag))),
            Self::Typed { layout, words } => {
                if words.is_empty() {
                    return None;
                }
                let (first, rest) = words.split_at(layout.stride_words());
                *words = rest;
                Some(Cow::Owned(decode_typed(layout, first)))
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }

    /// Steps over `n` elements without building them, so `skip` costs the
    /// same however far it skips.
    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        match self {
            Self::Values(values) => values.nth(n).map(Cow::Borrowed),
            Self::Int64(values) => values.nth(n).map(|bits| Cow::Owned(int64_value(*bits))),
            Self::Float64(values) => values.nth(n).map(|float| Cow::Owned(Value::Float(*float))),
            Self::Bool(values) => values.nth(n).map(|flag| Cow::Owned(Value::Bool(*flag))),
            Self::Typed { layout, words } => {
                let skipped = n.saturating_mul(layout.stride_words()).min(words.len());
                *words = &words[skipped..];
                self.next()
            }
        }
    }
}

impl DoubleEndedIterator for Iter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.nth_back(0)
    }

    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        match self {
            Self::Values(values) => values.nth_back(n).map(Cow::Borrowed),
            Self::Int64(values) => values
                .nth_back(n)
                .map(|bits| Cow::Owned(int64_value(*bits))),
            Self::Float64(values) => values
                .nth_back(n)
                .map(|float| Cow::Owned(Value::Float(*float))),
            Self::Bool(values) => values
                .nth_back(n)
                .map(|flag| Cow::Owned(Value::Bool(*flag))),
            Self::Typed { layout, words } => {
                if words.is_empty() {
                    return None;
                }
                let (rest, last) = words.split_at(words.len() - layout.stride_words());
                *words = rest;
                Some(Cow::Owned(decode_typed(layout, last)))
            }
        }
    }
}

impl ExactSizeIterator for Iter<'_> {
    #[inline]
    fn len(&self) -> usize {
        match self {
            Self::Values(values) => values.len(),
            Self::Int64(values) => values.len(),
            Self::Float64(values) => values.len(),
            Self::Bool(values) => values.len(),
            Self::Typed { layout, words } => words.len() / layout.stride(),
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
