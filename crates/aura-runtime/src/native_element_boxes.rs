//! The boxes a list keeps its elements in once generated code lends them.
//!
//! Reading an element of a list of strings, collections, class instances,
//! or enums used to copy the element into a new box. A list in the
//! [`ListElements::Boxes`](crate::list_elements::ListElements::Boxes) form
//! keeps each element in a box of its own instead, so a read can lend the
//! box to generated code and copy nothing (see `aura_direct_vec_lend`).
//!
//! The list owns the only reference to each box, and no task's ledger links
//! it. Only code that holds the list reaches the boxes, under the list's
//! own access (its lock once the list is shared), so a box takes no lock of
//! its own and records no thread. Two lists may share storage of boxes
//! until either changes, as with every form, but generated code reads a
//! box only while its list is confined to the thread and shares its
//! storage with no other list, and only until the list changes. A local
//! that must keep the element longer copies it. So a box is never read on
//! one thread while another thread changes it or its count.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use super::{element_boxed_value, release_untracked_value, OpaqueValue};
use crate::runtime_value::Value;

/// One element of a list in the `Boxes` form: a box the list owns.
pub struct ElementBox(NonNull<OpaqueValue>);

// SAFETY: a list's boxes are reached through the list, under the list's
// access, and generated code reads one only while the list is confined to
// its thread. The reference count is atomic.
unsafe impl Send for ElementBox {}
unsafe impl Sync for ElementBox {}

impl ElementBox {
    pub fn new(value: Value) -> Self {
        let ptr = element_boxed_value(value);
        Self(NonNull::new(ptr).expect("a new box is never null"))
    }

    /// The box, which generated code reads the element through.
    pub fn as_ptr(&self) -> *mut OpaqueValue {
        self.0.as_ptr()
    }

    fn header(&self) -> &OpaqueValue {
        // SAFETY: the list owns a reference, so the box is live.
        unsafe { self.0.as_ref() }
    }

    pub fn value(&self) -> &Value {
        // SAFETY: the list's access covers the box, and a write takes
        // `&mut self`, so no `&mut Value` exists while this borrow lives.
        unsafe { &*self.header().value.get() }
    }

    /// Whether anything holds a reference to the box besides the list, or
    /// another thread may read it, so the list must not change it in place.
    /// Generated code takes no reference to a box, so this only guards the
    /// rule.
    fn is_shared(&self) -> bool {
        self.header().ref_count.load(Ordering::Acquire) > 1 || self.header().shared_lock().is_some()
    }

    /// The element, to change in place. When anything else holds a
    /// reference to the box, that reference keeps the old value and the
    /// list takes a new box with a copy. The box's list view follows the
    /// change when the returned guard drops, so generated code that reads
    /// the element's own elements in place sees them.
    pub fn value_mut(&mut self) -> ElementMut<'_> {
        if self.is_shared() {
            *self = Self::new(self.value().clone());
        }
        ElementMut(self)
    }

    /// The element: moved out of the box when the list holds the only
    /// reference, and copied otherwise.
    pub fn into_value(self) -> Value {
        if self.is_shared() {
            return self.value().clone();
        }
        // SAFETY: the list holds the only reference, so nothing else reads
        // the value, and the box is freed below.
        unsafe { std::mem::replace(&mut *self.header().value.get(), Value::Unit) }
    }
}

impl Clone for ElementBox {
    /// A copy of the element in a new box, so two lists never share a box.
    fn clone(&self) -> Self {
        Self::new(self.value().clone())
    }
}

impl Drop for ElementBox {
    fn drop(&mut self) {
        // SAFETY: the list owns one reference to the box.
        unsafe { release_untracked_value(self.as_ptr()) }
    }
}

impl PartialEq for ElementBox {
    fn eq(&self, other: &Self) -> bool {
        self.value() == other.value()
    }
}

impl fmt::Debug for ElementBox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(formatter)
    }
}

/// An element being changed in its box. Dropping it brings the box's list
/// view up to date.
pub struct ElementMut<'a>(&'a mut ElementBox);

impl Deref for ElementMut<'_> {
    type Target = Value;

    fn deref(&self) -> &Value {
        self.0.value()
    }
}

impl DerefMut for ElementMut<'_> {
    fn deref_mut(&mut self) -> &mut Value {
        // SAFETY: the guard borrows the box mutably, and only the list holds
        // a reference to it (see `ElementBox::value_mut`).
        unsafe { &mut *self.0.header().value.get() }
    }
}

impl Drop for ElementMut<'_> {
    fn drop(&mut self) {
        self.0.header().refresh_list_view();
    }
}

#[cfg(test)]
#[path = "native_element_boxes_tests.rs"]
mod tests;
