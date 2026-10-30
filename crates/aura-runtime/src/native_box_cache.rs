//! A small cache of freed value boxes for each thread.
//!
//! Programs box and free values at a high rate, and the allocator's own
//! per-thread cache keeps only a few blocks of each size. A box freed here
//! goes back on this thread's list, and the next box this thread makes
//! takes it from there without calling the allocator. The list is bounded,
//! so a burst of frees hands the rest back to the allocator.

use std::alloc::{dealloc, Layout};
use std::cell::Cell;
use std::ptr;

use super::OpaqueValue;

/// The most boxes one thread keeps.
const CAPACITY: usize = 1024;

/// A cached box, linked through its first word.
struct FreeBox {
    next: *mut FreeBox,
}

struct BoxCache {
    head: Cell<*mut FreeBox>,
    len: Cell<usize>,
}

impl BoxCache {
    fn pop(&self) -> Option<*mut OpaqueValue> {
        let head = self.head.get();
        if head.is_null() {
            return None;
        }
        // SAFETY: a cached box is ours until it leaves the list.
        self.head.set(unsafe { (*head).next });
        self.len.set(self.len.get() - 1);
        Some(head.cast())
    }

    fn push(&self, value: *mut OpaqueValue) -> bool {
        if self.len.get() == CAPACITY {
            return false;
        }
        let free = value.cast::<FreeBox>();
        // SAFETY: the caller hands over a box whose value is dropped, and a
        // box is large enough and aligned for the link.
        unsafe {
            free.write(FreeBox {
                next: self.head.get(),
            })
        };
        self.head.set(free);
        self.len.set(self.len.get() + 1);
        true
    }
}

impl Drop for BoxCache {
    fn drop(&mut self) {
        while let Some(value) = self.pop() {
            // SAFETY: every cached box came from `Box<OpaqueValue>`.
            unsafe { dealloc(value.cast(), Layout::new::<OpaqueValue>()) };
        }
    }
}

const _: () = assert!(
    size_of::<OpaqueValue>() >= size_of::<FreeBox>()
        && align_of::<OpaqueValue>() >= align_of::<FreeBox>()
);

thread_local! {
    static BOX_CACHE: BoxCache = const {
        BoxCache {
            head: Cell::new(ptr::null_mut()),
            len: Cell::new(0),
        }
    };
}

/// Moves `value` into a box, reusing one this thread cached when it can.
pub(super) fn allocate(value: OpaqueValue) -> *mut OpaqueValue {
    match BOX_CACHE.try_with(BoxCache::pop).ok().flatten() {
        Some(slot) => {
            // SAFETY: a cached box has the layout of `OpaqueValue` and holds
            // no value.
            unsafe { slot.write(value) };
            slot
        }
        None => Box::into_raw(Box::new(value)),
    }
}

/// Drops the value in `value`, then caches its box or frees it.
///
/// # Safety
///
/// `value` came from [`allocate`], and the caller holds its last reference.
pub(super) unsafe fn free(value: *mut OpaqueValue) {
    // SAFETY: the caller's contract.
    unsafe { ptr::drop_in_place(value) };
    if !BOX_CACHE
        .try_with(|cache| cache.push(value))
        .unwrap_or(false)
    {
        // SAFETY: as in `BoxCache::drop`.
        unsafe { dealloc(value.cast(), Layout::new::<OpaqueValue>()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_value::Value;

    #[test]
    fn a_freed_box_is_reused_until_the_cache_is_full() {
        let first = allocate(OpaqueValue::new(Value::Unit, None));
        // SAFETY: the test holds the only reference.
        unsafe { free(first) };
        let second = allocate(OpaqueValue::new(Value::Bool(true), None));
        assert_eq!(first, second, "the cached box comes back");
        // SAFETY: as above.
        unsafe { free(second) };

        let boxes = (0..CAPACITY + 8)
            .map(|_| allocate(OpaqueValue::new(Value::Unit, None)))
            .collect::<Vec<_>>();
        for value in &boxes {
            // SAFETY: as above.
            unsafe { free(*value) };
        }
        let cached = BOX_CACHE.with(|cache| cache.len.get());
        assert_eq!(cached, CAPACITY, "the rest go back to the allocator");
    }
}
