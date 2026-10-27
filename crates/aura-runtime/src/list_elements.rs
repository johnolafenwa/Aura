//! Storage for the elements of a `list`.
//!
//! A list whose elements are all of one fixed-width scalar type keeps the
//! scalars themselves: each `int8`..`int64` or `uint8`..`uint64` element
//! in its own width, each `float32` or `float64` element in 8 bytes, and
//! each `bool` in 1 byte, instead of a whole `Value`. Any other list keeps
//! `Value`s. The storage follows the elements alone, and moving between
//! the forms loses nothing, so a program cannot observe which one a list
//! uses. Generated code reads and writes the scalar forms in place.
//!
//! Every form sits in a [`SharedVec`], so a clone shares the storage and the
//! first write through a shared handle copies it.

use std::borrow::Cow;
use std::collections::TryReserveError;
use std::fmt;

use crate::collection_index::SharedVec;
use crate::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use crate::randomness::DeterministicRng;
use crate::runtime_value::Value;

/// The elements of a `list`, in one of the forms the module docs describe.
#[derive(Clone)]
pub enum ListElements {
    Values(SharedVec<Value>),
    Int64(SharedVec<i64>),
    Float64(SharedVec<f64>),
    Bool(SharedVec<bool>),
    Int8(SharedVec<i8>),
    Int16(SharedVec<i16>),
    Int32(SharedVec<i32>),
    Uint8(SharedVec<u8>),
    Uint16(SharedVec<u16>),
    Uint32(SharedVec<u32>),
    Uint64(SharedVec<u64>),
}

/// Which form a [`ListElements`] uses. The discriminants are the native
/// list access codes for the same layouts, so a list view converts its form
/// without a table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ListForm {
    Values = 1,
    Int64 = 2,
    Float64 = 3,
    Bool = 4,
    Int8 = 6,
    Int16 = 7,
    Int32 = 8,
    Uint8 = 9,
    Uint16 = 10,
    Uint32 = 11,
    Uint64 = 12,
}

/// A scalar one of the forms keeps unboxed.
trait Element: Copy + PartialEq {
    /// The value this element stands for.
    fn value(self) -> Value;

    /// The element that stands for `value`, when this form keeps `value`
    /// exactly: the same type, and nothing [`Element::value`] would not
    /// rebuild.
    fn of(value: &Value) -> Option<Self>;

    /// The element equal to `value` as `==` compares, when there is one.
    fn equal_to(value: &Value) -> Option<Self> {
        Self::of(value)
    }
}

impl Element for f64 {
    fn value(self) -> Value {
        Value::Float(self)
    }

    fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Float(float) => Some(*float),
            _ => None,
        }
    }
}

impl Element for bool {
    fn value(self) -> Value {
        Value::Bool(self)
    }

    fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(flag) => Some(*flag),
            _ => None,
        }
    }
}

macro_rules! integer_elements {
    ($($ty:ty => $kind:ident, $element:ident => $build:expr;)*) => {$(
        impl Element for $ty {
            #[inline]
            fn value(self) -> Value {
                let $element = self;
                Value::Int($build)
            }

            #[inline]
            fn of(value: &Value) -> Option<Self> {
                let Value::Int(integer) = value else {
                    return None;
                };
                if integer.runtime_kind() != Some(IntegerKind::$kind) {
                    return None;
                }
                // The constructors keep a value at or above zero as
                // `Unsigned` and a negative one as `Signed`, so only that
                // representation is rebuilt exactly.
                match integer.representation() {
                    IntegerRepresentation::Unsigned(value) => <$ty>::try_from(value).ok(),
                    IntegerRepresentation::Signed(value) if value < 0 => {
                        <$ty>::try_from(value).ok()
                    }
                    IntegerRepresentation::Signed(_) => None,
                }
            }

            #[inline]
            fn equal_to(value: &Value) -> Option<Self> {
                match value {
                    Value::Int(integer) => <$ty>::try_from(integer.as_i128()?).ok(),
                    _ => None,
                }
            }
        }
    )*};
}

integer_elements! {
    i64 => Int64, element => IntegerValue::from_i64(element);
    i8 => Int8, element => signed(element.into(), IntegerKind::Int8);
    i16 => Int16, element => signed(element.into(), IntegerKind::Int16);
    i32 => Int32, element => IntegerValue::from_i32(element);
    u8 => Uint8, element => unsigned(element.into(), IntegerKind::Uint8);
    u16 => Uint16, element => unsigned(element.into(), IntegerKind::Uint16);
    u32 => Uint32, element => unsigned(element.into(), IntegerKind::Uint32);
    u64 => Uint64, element => IntegerValue::from_u64(element);
}

fn signed(value: i128, kind: IntegerKind) -> IntegerValue {
    IntegerValue::from_typed_signed(value, kind).expect("every element fits its integer kind")
}

fn unsigned(value: u128, kind: IntegerKind) -> IntegerValue {
    IntegerValue::from_typed_unsigned(value, kind).expect("every element fits its integer kind")
}

/// The bits of an `int64` value, when [`IntegerValue::from_i64`] rebuilds
/// exactly the same value from them. Integers of other kinds, and untyped
/// integer literals, stay boxed.
pub fn int64_bits(value: IntegerValue) -> Option<i64> {
    i64::of(&Value::Int(value))
}

/// Matches `$list` against every form, binding the storage to `$values`.
/// `$body` runs once per form, with `$values` a `SharedVec` of that form's
/// element type.
macro_rules! each_form {
    ($list:expr, $values:ident => $body:expr) => {
        match $list {
            ListElements::Values($values) => $body,
            ListElements::Int64($values) => $body,
            ListElements::Float64($values) => $body,
            ListElements::Bool($values) => $body,
            ListElements::Int8($values) => $body,
            ListElements::Int16($values) => $body,
            ListElements::Int32($values) => $body,
            ListElements::Uint8($values) => $body,
            ListElements::Uint16($values) => $body,
            ListElements::Uint32($values) => $body,
            ListElements::Uint64($values) => $body,
        }
    };
}

/// Matches `$list` against the scalar forms, binding the storage to
/// `$values` for `$body`, and runs `$fallback` for the `Value` form.
macro_rules! each_scalar_form {
    ($list:expr, $values:ident => $body:expr, $other:pat => $fallback:expr) => {
        match $list {
            ListElements::Int64($values) => $body,
            ListElements::Float64($values) => $body,
            ListElements::Bool($values) => $body,
            ListElements::Int8($values) => $body,
            ListElements::Int16($values) => $body,
            ListElements::Int32($values) => $body,
            ListElements::Uint8($values) => $body,
            ListElements::Uint16($values) => $body,
            ListElements::Uint32($values) => $body,
            ListElements::Uint64($values) => $body,
            $other => $fallback,
        }
    };
}

/// Matches a pair of lists that use the same scalar form, binding their
/// storage to `$left` and `$right` for `$body`, and runs `$fallback` for
/// any other pair.
macro_rules! same_scalar_forms {
    ($pair:expr, ($left:ident, $right:ident) => $body:expr, $other:pat => $fallback:expr) => {
        match $pair {
            (ListElements::Int64($left), ListElements::Int64($right)) => $body,
            (ListElements::Float64($left), ListElements::Float64($right)) => $body,
            (ListElements::Bool($left), ListElements::Bool($right)) => $body,
            (ListElements::Int8($left), ListElements::Int8($right)) => $body,
            (ListElements::Int16($left), ListElements::Int16($right)) => $body,
            (ListElements::Int32($left), ListElements::Int32($right)) => $body,
            (ListElements::Uint8($left), ListElements::Uint8($right)) => $body,
            (ListElements::Uint16($left), ListElements::Uint16($right)) => $body,
            (ListElements::Uint32($left), ListElements::Uint32($right)) => $body,
            (ListElements::Uint64($left), ListElements::Uint64($right)) => $body,
            $other => $fallback,
        }
    };
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
#[inline]
unsafe fn adopt_appended<T>(values: &mut SharedVec<T>, len: usize) {
    if let Some(values) = values.unshared_mut() {
        if len > values.len() && len <= values.capacity() {
            // SAFETY: the caller wrote each element up to `len`.
            unsafe { values.set_len(len) };
        }
    }
}

/// The scalar form that keeps `value` exactly, if any.
fn scalar_form_of(value: &Value) -> Option<ListForm> {
    match value {
        Value::Float(_) => Some(ListForm::Float64),
        Value::Bool(_) => Some(ListForm::Bool),
        Value::Int(integer) => {
            let form = match integer.runtime_kind()? {
                IntegerKind::Int64 => ListForm::Int64,
                IntegerKind::Int8 => ListForm::Int8,
                IntegerKind::Int16 => ListForm::Int16,
                IntegerKind::Int32 => ListForm::Int32,
                IntegerKind::Uint8 => ListForm::Uint8,
                IntegerKind::Uint16 => ListForm::Uint16,
                IntegerKind::Uint32 => ListForm::Uint32,
                IntegerKind::Uint64 => ListForm::Uint64,
                IntegerKind::Int128
                | IntegerKind::IntSize
                | IntegerKind::Uint128
                | IntegerKind::UintSize => return None,
            };
            // A value with a representation the form would not rebuild
            // stays boxed.
            keeps(form, value).then_some(form)
        }
        _ => None,
    }
}

/// Whether `form` keeps `value` exactly.
fn keeps(form: ListForm, value: &Value) -> bool {
    match form {
        ListForm::Values => true,
        ListForm::Int64 => i64::of(value).is_some(),
        ListForm::Float64 => f64::of(value).is_some(),
        ListForm::Bool => bool::of(value).is_some(),
        ListForm::Int8 => i8::of(value).is_some(),
        ListForm::Int16 => i16::of(value).is_some(),
        ListForm::Int32 => i32::of(value).is_some(),
        ListForm::Uint8 => u8::of(value).is_some(),
        ListForm::Uint16 => u16::of(value).is_some(),
        ListForm::Uint32 => u32::of(value).is_some(),
        ListForm::Uint64 => u64::of(value).is_some(),
    }
}

/// `values` as one form's elements, when the form keeps every one exactly.
fn collect_form<T: Element>(values: &[Value]) -> Option<SharedVec<T>> {
    values
        .iter()
        .map(T::of)
        .collect::<Option<Vec<_>>>()
        .map(SharedVec::from)
}

impl ListElements {
    pub fn new() -> Self {
        Self::Values(SharedVec::new())
    }

    /// A `list[uint8]` that keeps `bytes` unboxed.
    pub fn from_uint8(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            Self::new()
        } else {
            Self::Uint8(bytes.into())
        }
    }

    /// The elements as bytes, when the list keeps `uint8` elements unboxed.
    pub fn as_uint8(&self) -> Option<&[u8]> {
        match self {
            Self::Uint8(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// `values` in `form`, when the form keeps every one exactly.
    fn collect(form: ListForm, values: &[Value]) -> Option<Self> {
        Some(match form {
            ListForm::Values => Self::Values(values.to_vec().into()),
            ListForm::Int64 => Self::Int64(collect_form(values)?),
            ListForm::Float64 => Self::Float64(collect_form(values)?),
            ListForm::Bool => Self::Bool(collect_form(values)?),
            ListForm::Int8 => Self::Int8(collect_form(values)?),
            ListForm::Int16 => Self::Int16(collect_form(values)?),
            ListForm::Int32 => Self::Int32(collect_form(values)?),
            ListForm::Uint8 => Self::Uint8(collect_form(values)?),
            ListForm::Uint16 => Self::Uint16(collect_form(values)?),
            ListForm::Uint32 => Self::Uint32(collect_form(values)?),
            ListForm::Uint64 => Self::Uint64(collect_form(values)?),
        })
    }

    #[inline]
    pub fn form(&self) -> ListForm {
        match self {
            Self::Values(_) => ListForm::Values,
            Self::Int64(_) => ListForm::Int64,
            Self::Float64(_) => ListForm::Float64,
            Self::Bool(_) => ListForm::Bool,
            Self::Int8(_) => ListForm::Int8,
            Self::Int16(_) => ListForm::Int16,
            Self::Int32(_) => ListForm::Int32,
            Self::Uint8(_) => ListForm::Uint8,
            Self::Uint16(_) => ListForm::Uint16,
            Self::Uint32(_) => ListForm::Uint32,
            Self::Uint64(_) => ListForm::Uint64,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        each_form!(self, values => values.len())
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
        #[inline]
        fn scalar<T>(form: ListForm, values: &SharedVec<T>) -> (ListForm, usize, usize, usize) {
            let room = unshared_capacity(values);
            (form, values.as_ptr() as usize, values.len(), room)
        }
        match self {
            Self::Values(values) => (ListForm::Values, values.as_ptr() as usize, values.len(), 0),
            Self::Int64(values) => scalar(ListForm::Int64, values),
            Self::Float64(values) => scalar(ListForm::Float64, values),
            Self::Bool(values) => scalar(ListForm::Bool, values),
            Self::Int8(values) => scalar(ListForm::Int8, values),
            Self::Int16(values) => scalar(ListForm::Int16, values),
            Self::Int32(values) => scalar(ListForm::Int32, values),
            Self::Uint8(values) => scalar(ListForm::Uint8, values),
            Self::Uint16(values) => scalar(ListForm::Uint16, values),
            Self::Uint32(values) => scalar(ListForm::Uint32, values),
            Self::Uint64(values) => scalar(ListForm::Uint64, values),
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
    #[inline]
    pub unsafe fn adopt_appended(&mut self, len: usize) {
        each_scalar_form!(
            self,
            values => unsafe { adopt_appended(values, len) },
            Self::Values(_) => {}
        )
    }

    /// Whether another handle shares this storage.
    pub fn is_shared(&self) -> bool {
        each_form!(self, values => values.is_shared())
    }

    /// The address of the first element. Two lists with the same address
    /// share their storage.
    #[inline]
    pub fn as_ptr(&self) -> usize {
        each_form!(self, values => values.as_ptr() as usize)
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<Cow<'_, Value>> {
        each_scalar_form!(
            self,
            values => values.get(index).map(|element| Cow::Owned(element.value())),
            Self::Values(values) => values.get(index).map(Cow::Borrowed)
        )
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
            Self::Int8(values) => Iter::Int8(values.iter()),
            Self::Int16(values) => Iter::Int16(values.iter()),
            Self::Int32(values) => Iter::Int32(values.iter()),
            Self::Uint8(values) => Iter::Uint8(values.iter()),
            Self::Uint16(values) => Iter::Uint16(values.iter()),
            Self::Uint32(values) => Iter::Uint32(values.iter()),
            Self::Uint64(values) => Iter::Uint64(values.iter()),
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
        let pushed = each_scalar_form!(
            &mut *self,
            values => match Element::of(&value) {
                Some(element) => {
                    values.push(element);
                    true
                }
                None => false,
            },
            Self::Values(_) => false
        );
        if pushed {
            return;
        }
        if self.is_empty() {
            match scalar_form_of(&value) {
                Some(form) => {
                    *self = Self::collect(form, std::slice::from_ref(&value))
                        .expect("the form keeps the value it was picked for");
                    return;
                }
                // An empty `Values` list keeps its capacity, such as after
                // `clear`.
                None if !matches!(self, Self::Values(_)) => {
                    *self = Self::Values(vec![value].into());
                    return;
                }
                None => {}
            }
        }
        self.values_mut().push(value);
    }

    pub fn pop(&mut self) -> Option<Value> {
        each_scalar_form!(
            self,
            values => values.pop().map(Element::value),
            Self::Values(values) => values.pop()
        )
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
        let replaced = each_scalar_form!(
            &mut *self,
            values => Element::of(&value)
                .map(|element| std::mem::replace(&mut values[index], element).value()),
            Self::Values(_) => None
        );
        match replaced {
            Some(previous) => previous,
            None => std::mem::replace(&mut self.values_mut()[index], value),
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
        let inserted = each_scalar_form!(
            &mut *self,
            values => match Element::of(&value) {
                Some(element) => {
                    values.insert(index, element);
                    true
                }
                None => false,
            },
            Self::Values(_) => false
        );
        if inserted {
            return;
        }
        if self.is_empty() {
            self.push(value);
        } else {
            self.values_mut().insert(index, value);
        }
    }

    /// Removes and returns the element at `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is out of range.
    pub fn remove(&mut self, index: usize) -> Value {
        let len = self.len();
        assert!(
            index < len,
            "list remove index {index} is out of range for length {len}"
        );
        each_scalar_form!(
            self,
            values => values.remove(index).value(),
            Self::Values(values) => values.remove(index)
        )
    }

    pub fn swap(&mut self, first: usize, second: usize) {
        each_form!(self, values => values.swap(first, second))
    }

    pub fn reverse(&mut self) {
        each_form!(self, values => values.reverse())
    }

    pub fn clear(&mut self) {
        // A fresh empty list drops shared storage without copying it.
        *self = Self::new();
    }

    pub fn truncate(&mut self, len: usize) {
        if len >= self.len() {
            return;
        }
        each_form!(self, values => values.truncate(len))
    }

    /// Appends every element of `other`.
    pub fn append(&mut self, other: ListElements) {
        if self.is_empty() {
            *self = other;
            return;
        }
        same_scalar_forms!(
            (&mut *self, other),
            (values, more) => values.extend_from_slice(&more),
            (list, other) => {
                for value in other {
                    list.push(value);
                }
            }
        )
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
            Self::Int8(values) => Self::Int8(values.drain(start..end).collect()),
            Self::Int16(values) => Self::Int16(values.drain(start..end).collect()),
            Self::Int32(values) => Self::Int32(values.drain(start..end).collect()),
            Self::Uint8(values) => Self::Uint8(values.drain(start..end).collect()),
            Self::Uint16(values) => Self::Uint16(values.drain(start..end).collect()),
            Self::Uint32(values) => Self::Uint32(values.drain(start..end).collect()),
            Self::Uint64(values) => Self::Uint64(values.drain(start..end).collect()),
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
        same_scalar_forms!(
            (&mut *self, other),
            (values, more) => {
                values.splice(index..index, more.iter().copied());
            },
            (list, other) => {
                list.update(|values| {
                    values.splice(index..index, other);
                });
            }
        )
    }

    pub fn capacity(&self) -> usize {
        each_form!(self, values => values.capacity())
    }

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        each_form!(self, values => values.try_reserve(additional))
    }

    /// Shuffles the elements in place with `rng`.
    pub fn shuffle(&mut self, rng: &mut DeterministicRng) {
        each_form!(self, values => rng.shuffle(values))
    }

    pub fn contains(&self, needle: &Value) -> bool {
        let found = each_scalar_form!(
            self,
            values => Element::equal_to(needle).map(|element| values.contains(&element)),
            Self::Values(_) => None
        );
        found.unwrap_or_else(|| self.iter().any(|element| *element == *needle))
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
        values
            .first()
            .and_then(scalar_form_of)
            .and_then(|form| Self::collect(form, &values))
            .unwrap_or_else(|| Self::Values(values.into()))
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
        same_scalar_forms!(
            (self, other),
            (left, right) => left[..] == right[..],
            // No pointer shortcut: a list holding NaN is not equal to itself.
            _ => {
                self.len() == other.len()
                    && self
                        .iter()
                        .zip(other.iter())
                        .all(|(left, right)| left == right)
            }
        )
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
    Int8(std::slice::Iter<'a, i8>),
    Int16(std::slice::Iter<'a, i16>),
    Int32(std::slice::Iter<'a, i32>),
    Uint8(std::slice::Iter<'a, u8>),
    Uint16(std::slice::Iter<'a, u16>),
    Uint32(std::slice::Iter<'a, u32>),
    Uint64(std::slice::Iter<'a, u64>),
}

/// Matches `$iter` against every scalar iterator, binding it to
/// `$values`, and runs `$fallback` for the `Value` iterator.
macro_rules! each_scalar_iter {
    ($iter:expr, $values:ident => $body:expr, $other:pat => $fallback:expr) => {
        match $iter {
            Iter::Int64($values) => $body,
            Iter::Float64($values) => $body,
            Iter::Bool($values) => $body,
            Iter::Int8($values) => $body,
            Iter::Int16($values) => $body,
            Iter::Int32($values) => $body,
            Iter::Uint8($values) => $body,
            Iter::Uint16($values) => $body,
            Iter::Uint32($values) => $body,
            Iter::Uint64($values) => $body,
            $other => $fallback,
        }
    };
}

impl<'a> Iterator for Iter<'a> {
    type Item = Cow<'a, Value>;

    fn next(&mut self) -> Option<Self::Item> {
        each_scalar_iter!(
            self,
            values => values.next().map(|element| Cow::Owned(element.value())),
            Self::Values(values) => values.next().map(Cow::Borrowed)
        )
    }

    /// Skips `n` elements without building them, so `skip` on a long list
    /// costs one step.
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        each_scalar_iter!(
            self,
            values => values.nth(n).map(|element| Cow::Owned(element.value())),
            Self::Values(values) => values.nth(n).map(Cow::Borrowed)
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        each_scalar_iter!(
            self,
            values => values.next_back().map(|element| Cow::Owned(element.value())),
            Self::Values(values) => values.next_back().map(Cow::Borrowed)
        )
    }

    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        each_scalar_iter!(
            self,
            values => values.nth_back(n).map(|element| Cow::Owned(element.value())),
            Self::Values(values) => values.nth_back(n).map(Cow::Borrowed)
        )
    }
}

impl ExactSizeIterator for Iter<'_> {
    fn len(&self) -> usize {
        each_scalar_iter!(
            self,
            values => values.len(),
            Self::Values(values) => values.len()
        )
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
