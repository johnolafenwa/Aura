//! The references to boxed values that one task owns, which the runtime
//! releases when the task fails or is cancelled.
//!
//! A task's ledger lives in its [`DirectFrameStack`], whose address names
//! the task. A value confined to its thread links into the ledger of the
//! task that first owns it: the value's header names that stack, counts the
//! references the task owns, and holds the list links. Registering and
//! releasing such a reference changes the header and needs no lookup, and
//! generated code does the same in place while the count stays above one.
//!
//! A shared value never links, because another thread may read its header.
//! Neither does a value that another task's ledger already links. The ledger
//! counts those references by address instead.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::ptr;
use std::sync::atomic::Ordering;

use super::{AddressHasher, DirectFrameStack, OpaqueValue};
use crate::runtime_value::representation_stats;

type OtherReferences = HashMap<usize, usize, BuildHasherDefault<AddressHasher>>;

/// The references one task owns.
pub(super) struct DirectOwnedValues {
    /// Whether the task keeps a ledger at all.
    tracking: bool,
    /// The most recently linked value, or null.
    head: *mut OpaqueValue,
    /// References to values this ledger does not link, by address.
    others: OtherReferences,
}

impl DirectOwnedValues {
    pub(super) fn new() -> Self {
        Self {
            tracking: false,
            head: ptr::null_mut(),
            others: OtherReferences::default(),
        }
    }

    pub(super) fn tracking(&self) -> bool {
        self.tracking
    }

    pub(super) fn set_tracking(&mut self, tracking: bool) {
        self.tracking = tracking;
    }

    /// Whether the task owns no reference.
    pub(super) fn is_empty(&self) -> bool {
        self.head.is_null() && self.others.is_empty()
    }

    /// The references the task owns to `value`.
    #[cfg(test)]
    pub(super) fn count(&self, value: *mut OpaqueValue) -> usize {
        let address = value as usize;
        self.entries()
            .into_iter()
            .filter(|(entry, _)| *entry == address)
            .map(|(_, count)| count)
            .sum()
    }

    /// Every owned value's address with its reference count.
    pub(super) fn entries(&self) -> Vec<(usize, usize)> {
        let mut entries = Vec::new();
        let mut value = self.head;
        while let Some(current) = unsafe { value.as_ref() } {
            entries.push((value as usize, current.owned.get() as usize));
            value = current.next.get();
        }
        entries.extend(
            self.others
                .iter()
                .map(|(address, count)| (*address, *count)),
        );
        entries
    }

    /// Adds one reference to `value`, which the task of `owner` now owns.
    /// Returns false when the count would overflow.
    ///
    /// # Safety
    ///
    /// `value` is a live box this thread may reach, and `owner` is the stack
    /// that holds this ledger.
    pub(super) unsafe fn add(
        &mut self,
        owner: *mut DirectFrameStack,
        value: *mut OpaqueValue,
    ) -> bool {
        // SAFETY: the caller passes a live box.
        let header = unsafe { &*value };
        // While the ownership counters count, every retain and release has
        // to reach the runtime, so no value links and generated code never
        // takes its own path.
        if header.shared_lock.load(Ordering::Acquire).is_null()
            && !representation_stats::ownership_counting()
        {
            // The value is confined to this thread, so its links are ours
            // to read.
            let linked = header.owner.get();
            if linked == owner {
                return match header.owned.get().checked_add(1) {
                    Some(count) => {
                        header.owned.set(count);
                        true
                    }
                    None => false,
                };
            }
            if linked.is_null() {
                header.owner.set(owner);
                header.owned.set(1);
                header.prev.set(ptr::null_mut());
                header.next.set(self.head);
                // SAFETY: a linked value is live and confined to this
                // thread.
                if let Some(head) = unsafe { self.head.as_ref() } {
                    head.prev.set(value);
                }
                self.head = value;
                return true;
            }
        }
        let count = self.others.entry(value as usize).or_default();
        match count.checked_add(1) {
            Some(next) => {
                *count = next;
                true
            }
            None => false,
        }
    }

    /// Removes one reference to `value` the task of `owner` owns. Returns
    /// whether it owned one.
    ///
    /// # Safety
    ///
    /// As for [`DirectOwnedValues::add`].
    pub(super) unsafe fn remove(
        &mut self,
        owner: *mut DirectFrameStack,
        value: *mut OpaqueValue,
    ) -> bool {
        // SAFETY: the caller passes a live box.
        let header = unsafe { &*value };
        if header.shared_lock.load(Ordering::Acquire).is_null() && header.owner.get() == owner {
            let count = header.owned.get() - 1;
            header.owned.set(count);
            if count == 0 {
                // SAFETY: the value is linked here.
                unsafe { self.unlink(header) };
            }
            return true;
        }
        let address = value as usize;
        let Some(count) = self.others.get_mut(&address) else {
            return false;
        };
        *count -= 1;
        if *count == 0 {
            self.others.remove(&address);
        }
        true
    }

    /// Removes the last reference to `value` when this ledger links it and
    /// owns exactly one. Returns false, changing nothing, otherwise.
    ///
    /// # Safety
    ///
    /// As for [`DirectOwnedValues::add`].
    pub(super) unsafe fn remove_last(
        &mut self,
        owner: *mut DirectFrameStack,
        value: *mut OpaqueValue,
    ) -> bool {
        if value.is_null() {
            return false;
        }
        // SAFETY: the caller passes a live box.
        let header = unsafe { &*value };
        if !header.shared_lock.load(Ordering::Acquire).is_null()
            || header.owner.get() != owner
            || header.owned.get() != 1
        {
            return false;
        }
        header.owned.set(0);
        // SAFETY: the value is linked here.
        unsafe { self.unlink(header) };
        true
    }

    /// Takes every reference, leaving the ledger empty. The caller releases
    /// each address its count of times.
    pub(super) fn take_all(&mut self) -> Vec<(usize, usize)> {
        let mut entries = Vec::new();
        while let Some(head) = unsafe { self.head.as_ref() } {
            entries.push((self.head as usize, head.owned.get() as usize));
            head.owned.set(0);
            // SAFETY: the head is linked here.
            unsafe { self.unlink(head) };
        }
        entries.extend(self.others.drain());
        entries
    }

    /// Installs the references a task starts with. The ledger must be empty,
    /// and `others` must already hold every entry, so this cannot fail
    /// halfway.
    pub(super) fn install_others(&mut self, others: OtherReferences) {
        self.others = others;
    }

    /// Takes `header` out of the list.
    ///
    /// # Safety
    ///
    /// `header` is linked into this ledger.
    unsafe fn unlink(&mut self, header: &OpaqueValue) {
        let prev = header.prev.get();
        let next = header.next.get();
        // SAFETY: linked neighbours are live and confined to this thread.
        match unsafe { prev.as_ref() } {
            Some(prev) => prev.next.set(next),
            None => self.head = next,
        }
        if let Some(next) = unsafe { next.as_ref() } {
            next.prev.set(prev);
        }
        header.owner.set(ptr::null_mut());
        header.prev.set(ptr::null_mut());
        header.next.set(ptr::null_mut());
    }
}

impl Drop for DirectOwnedValues {
    /// Forgets the references still listed without releasing them, so no
    /// value names a stack that is gone.
    fn drop(&mut self) {
        while let Some(head) = unsafe { self.head.as_ref() } {
            head.owned.set(0);
            // SAFETY: the head is linked here.
            unsafe { self.unlink(head) };
        }
    }
}

/// Moves the references a ledger links for `header` into that ledger's
/// count by address, before the value becomes shared and other threads may
/// read its header.
///
/// # Safety
///
/// The caller is the thread `header` is confined to.
pub(super) unsafe fn detach_for_sharing(value: *mut OpaqueValue, header: &OpaqueValue) {
    let owner = header.owner.get();
    if owner.is_null() {
        return;
    }
    // SAFETY: a linked value names the live stack of a task on this thread,
    // and no other reference to that stack's ledger is in use here.
    let ledger = unsafe { &mut (*owner).owned_values };
    let count = header.owned.get();
    header.owned.set(0);
    // SAFETY: the value is linked into `ledger`.
    unsafe { ledger.unlink(header) };
    *ledger.others.entry(value as usize).or_default() += count as usize;
}

/// Takes a value that is being freed out of the list that links it.
///
/// # Safety
///
/// As for [`detach_for_sharing`].
pub(super) unsafe fn detach_for_drop(header: &OpaqueValue) {
    let owner = header.owner.get();
    if owner.is_null() {
        return;
    }
    // SAFETY: as in `detach_for_sharing`.
    let ledger = unsafe { &mut (*owner).owned_values };
    // SAFETY: the value is linked into `ledger`.
    unsafe { ledger.unlink(header) };
}

/// Where generated code finds the reference counts in a boxed value. The
/// offsets hold for one build of the runtime, so the compiler records
/// [`DirectOwnershipLayout::fingerprint`] in each executable and
/// `aura_direct_check_ownership_layout` compares it at startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectOwnershipLayout {
    /// The offset of the `u32` count of all references.
    pub ref_count_offset: i32,
    /// The offset of the shared lock's address, which is zero while the
    /// value is confined to one thread.
    pub shared_lock_offset: i32,
    /// The offset of the address of the frame stack whose ledger links the
    /// value, or zero.
    pub owner_offset: i32,
    /// The offset of the `u32` count of references that ledger holds.
    pub owned_offset: i32,
}

impl DirectOwnershipLayout {
    /// One number that differs whenever an offset differs.
    pub fn fingerprint(&self) -> i64 {
        let fields = [
            i64::from(self.ref_count_offset),
            i64::from(self.shared_lock_offset),
            i64::from(self.owner_offset),
            i64::from(self.owned_offset),
            // The count widths: a 4-byte total and a 4-byte owned count.
            0x0404,
        ];
        fields.iter().fold(0x4f_77_6e_73_i64, |hash, field| {
            hash.wrapping_mul(0x100_0000_01b3).wrapping_add(*field)
        })
    }
}

/// The layout of this build of the runtime.
pub fn direct_ownership_layout() -> DirectOwnershipLayout {
    DirectOwnershipLayout {
        ref_count_offset: std::mem::offset_of!(OpaqueValue, ref_count) as i32,
        shared_lock_offset: std::mem::offset_of!(OpaqueValue, shared_lock) as i32,
        owner_offset: std::mem::offset_of!(OpaqueValue, owner) as i32,
        owned_offset: std::mem::offset_of!(OpaqueValue, owned) as i32,
    }
}

/// Retains `value` the way generated code does, when its rule allows.
/// Returns whether it did.
///
/// # Safety
///
/// `value` is a live box this thread may reach.
#[cfg(test)]
pub(super) unsafe fn generated_retain(
    stack: *mut DirectFrameStack,
    value: *mut OpaqueValue,
) -> bool {
    let layout = direct_ownership_layout();
    let base = value.cast::<u8>();
    // SAFETY: these are the reads and writes generated code makes.
    unsafe {
        let word = |offset: i32| base.offset(offset as isize).cast::<usize>();
        if word(layout.shared_lock_offset).read() != 0
            || word(layout.owner_offset).read() != stack as usize
        {
            return false;
        }
        let ref_count = base.offset(layout.ref_count_offset as isize).cast::<u32>();
        if ref_count.read() == u32::MAX {
            return false;
        }
        ref_count.write(ref_count.read() + 1);
        let owned = word(layout.owned_offset);
        owned.write(owned.read() + 1);
    }
    true
}

/// Releases `value` the way generated code does, when its rule allows.
/// Returns whether it did.
///
/// # Safety
///
/// As for [`generated_retain`].
#[cfg(test)]
pub(super) unsafe fn generated_release(
    stack: *mut DirectFrameStack,
    value: *mut OpaqueValue,
) -> bool {
    let layout = direct_ownership_layout();
    let base = value.cast::<u8>();
    // SAFETY: as in `generated_retain`.
    unsafe {
        let word = |offset: i32| base.offset(offset as isize).cast::<usize>();
        if word(layout.shared_lock_offset).read() != 0
            || word(layout.owner_offset).read() != stack as usize
        {
            return false;
        }
        let ref_count = base.offset(layout.ref_count_offset as isize).cast::<u32>();
        let owned = word(layout.owned_offset);
        if owned.read() <= 1 || ref_count.read() <= 1 {
            return false;
        }
        ref_count.write(ref_count.read() - 1);
        owned.write(owned.read() - 1);
    }
    true
}

#[cfg(test)]
#[path = "native_owned_values_tests.rs"]
mod tests;
