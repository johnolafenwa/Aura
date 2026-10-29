//! The stack of active Aura calls of one task, which generated code pushes
//! and pops in place.
//!
//! Each task owns one [`DirectFrameStack`] at a fixed heap address. A call
//! records one word: the address of its function's static
//! [`DirectFrameDescriptor`], or the address of a boxed frame record with
//! the low bit set. Generated code pushes and pops descriptor words itself
//! while the header allows it, and calls `aura_direct_enter_frame` or
//! `aura_direct_exit_call` otherwise. [`DirectFrameStackLayout`] tells the
//! compiler where the header fields are.
//!
//! The stack's address also names its task to the ownership ledger, which
//! the stack carries in [`DirectFrameStack::owned_values`].

use std::ptr::{self, NonNull};

use super::owned_values::DirectOwnedValues;
use super::{DirectFrameDescriptor, DirectRuntimeCallFrame};
use crate::diag::RuntimeCallFrame;

/// The most calls one task may have active at once.
pub(super) const DIRECT_MAX_CALL_DEPTH: usize = 256;

/// The entries a stack holds before it moves them to the heap.
pub(super) const DIRECT_FRAME_STACK_INLINE_ENTRIES: usize = 64;

/// The low bit of an entry that holds a boxed frame record instead of a
/// descriptor. Both are 8-byte aligned, so the bit is free.
const DIRECT_OWNED_FRAME_TAG: usize = 1;

/// A call handed mutable sinks to the next callee.
pub(super) const DIRECT_FRAME_FLAG_PENDING_SINKS: u64 = 1;
/// An active call holds a returned-view record.
pub(super) const DIRECT_FRAME_FLAG_RETURNED_VIEWS: u64 = 1 << 1;
/// The stack holds a boxed frame record, which only the runtime may pop.
const DIRECT_FRAME_FLAG_OWNED_FRAMES: u64 = 1 << 2;

/// The active calls of one task.
///
/// Generated code reads the first three fields and `entries`. It pushes a
/// call by storing the descriptor address at `entries[len]` and adding one
/// to `len`, when `len < limit`. It pops a call by subtracting one from
/// `len`, when `len > pop_floor`. While `flags` is nonzero, `limit` is 0
/// and `pop_floor` is `usize::MAX`, so each check sends the call to the
/// runtime with one compare. Every other push and pop goes through the
/// runtime, which keeps the header true for both rules.
/// The program validates every descriptor at startup, so an inline push
/// stores only validated ones.
#[repr(C)]
pub struct DirectFrameStack {
    /// The number of entries in use.
    len: usize,
    /// The smaller of the capacity and the depth limit above `floor`, or
    /// 0 while `flags` is nonzero.
    limit: usize,
    /// `floor`, or `usize::MAX` while `flags` is nonzero.
    pop_floor: usize,
    /// Nonzero while a push or pop has runtime work to do.
    flags: u64,
    /// The first entry: `inline`, or a heap block once the stack grows.
    entries: *mut usize,
    capacity: usize,
    runtime_flags: u64,
    owned_frames: usize,
    /// The entries that belong to a body that cleanup interrupted. Call
    /// depth counts from here, and a pop never goes below it.
    floor: usize,
    /// The heap block, which holds `capacity` entries when not null.
    spill: *mut usize,
    inline: [usize; DIRECT_FRAME_STACK_INLINE_ENTRIES],
    /// The references the task owns. A value the ledger links names this
    /// stack, so generated code compares the value's owner with the
    /// address it already holds.
    pub(super) owned_values: DirectOwnedValues,
}

impl DirectFrameStack {
    /// An empty stack at a fixed address, which [`DirectFrameStack::free`]
    /// releases.
    pub(super) fn allocate() -> NonNull<Self> {
        let stack = NonNull::from(Box::leak(Box::new(Self {
            len: 0,
            limit: 0,
            pop_floor: 0,
            flags: 0,
            entries: ptr::null_mut(),
            capacity: DIRECT_FRAME_STACK_INLINE_ENTRIES,
            runtime_flags: 0,
            owned_frames: 0,
            floor: 0,
            spill: ptr::null_mut(),
            inline: [0; DIRECT_FRAME_STACK_INLINE_ENTRIES],
            owned_values: DirectOwnedValues::new(),
        })));
        // SAFETY: the box was just leaked, so nothing else refers to it.
        let header = unsafe { &mut *stack.as_ptr() };
        header.entries = header.inline.as_mut_ptr();
        header.update_limit();
        stack
    }

    /// Releases `stack` and the frame records it still holds.
    ///
    /// # Safety
    ///
    /// `stack` came from [`DirectFrameStack::allocate`], and nothing uses it
    /// afterwards.
    pub(super) unsafe fn free(stack: NonNull<Self>) {
        // SAFETY: the caller hands over the only use of the allocation.
        let mut stack = unsafe { Box::from_raw(stack.as_ptr()) };
        stack.floor = 0;
        stack.update_limit();
        while let Some(word) = stack.pop() {
            release_frame_word(word);
        }
        stack.release_spill();
    }

    /// The calls above the floor.
    pub(super) fn depth(&self) -> usize {
        self.len - self.floor
    }

    /// Makes the depth `depth` without changing the entries: the entries
    /// below it become the floor.
    pub(super) fn set_depth(&mut self, depth: usize) {
        self.floor = self.len.saturating_sub(depth);
        self.update_limit();
    }

    /// The active entries, outermost first.
    pub(super) fn words(&self) -> &[usize] {
        // SAFETY: the first `len` entries of the current block are
        // initialized.
        unsafe { std::slice::from_raw_parts(self.block(), self.len) }
    }

    /// Pushes a descriptor's address.
    pub(super) fn push_descriptor(&mut self, descriptor: NonNull<DirectFrameDescriptor>) {
        self.push_word(descriptor.as_ptr() as usize);
    }

    /// Pushes a frame record the stack owns until it pops it.
    pub(super) fn push_owned(&mut self, frame: Box<DirectRuntimeCallFrame>) {
        self.owned_frames += 1;
        self.push_word(Box::into_raw(frame) as usize | DIRECT_OWNED_FRAME_TAG);
        self.update_flags();
    }

    fn push_word(&mut self, word: usize) {
        if self.len == self.capacity {
            self.grow();
        }
        // SAFETY: `len` is below the capacity of the current block.
        unsafe { self.block_mut().add(self.len).write(word) };
        self.len += 1;
    }

    /// Pops the innermost call above the floor, and releases its record.
    /// Returns whether a call was active.
    pub(super) fn pop_call(&mut self) -> bool {
        match self.pop() {
            Some(word) => {
                release_frame_word(word);
                true
            }
            None => false,
        }
    }

    fn pop(&mut self) -> Option<usize> {
        if self.len <= self.floor {
            return None;
        }
        self.len -= 1;
        let word = self.word_at(self.len);
        if word & DIRECT_OWNED_FRAME_TAG != 0 {
            self.owned_frames -= 1;
            self.update_flags();
        }
        Some(word)
    }

    fn word_at(&self, index: usize) -> usize {
        // SAFETY: callers pass an index below the old `len`, whose entries
        // are initialized.
        unsafe { self.block().add(index).read() }
    }

    /// Sets the runtime's own flag bits, which send every push and pop of
    /// generated code to the runtime while any is set.
    pub(super) fn set_runtime_flags(&mut self, flags: u64) {
        self.runtime_flags = flags;
        self.update_flags();
    }

    /// The active calls, innermost first.
    pub(super) fn materialize_innermost_first(&self) -> Vec<RuntimeCallFrame> {
        self.words()
            .iter()
            .rev()
            .map(|word| frame_from_word(*word).materialize())
            .collect()
    }

    fn update_flags(&mut self) {
        let owned = if self.owned_frames == 0 {
            0
        } else {
            DIRECT_FRAME_FLAG_OWNED_FRAMES
        };
        self.flags = self.runtime_flags | owned;
        self.update_limit();
    }

    /// Brings `limit` and `pop_floor` up to date with the fields they
    /// follow.
    fn update_limit(&mut self) {
        if self.flags == 0 {
            self.limit = self
                .capacity
                .min(self.floor.saturating_add(DIRECT_MAX_CALL_DEPTH));
            self.pop_floor = self.floor;
        } else {
            self.limit = 0;
            self.pop_floor = usize::MAX;
        }
    }

    fn block(&self) -> *const usize {
        if self.spill.is_null() {
            self.inline.as_ptr()
        } else {
            self.spill
        }
    }

    fn block_mut(&mut self) -> *mut usize {
        if self.spill.is_null() {
            self.inline.as_mut_ptr()
        } else {
            self.spill
        }
    }

    /// Moves the entries to a heap block twice the size. A stack keeps its
    /// largest block, so recursion that moves between depths does not
    /// allocate on each call.
    #[cold]
    fn grow(&mut self) {
        let capacity = self.capacity * 2;
        let mut block = std::mem::ManuallyDrop::new(Vec::<usize>::with_capacity(capacity));
        block.extend_from_slice(self.words());
        let spill = block.as_mut_ptr();
        self.release_spill();
        self.spill = spill;
        self.entries = spill;
        self.capacity = capacity;
        self.update_limit();
    }

    fn release_spill(&mut self) {
        if !self.spill.is_null() {
            // SAFETY: `spill` came from a `Vec` of `capacity` entries, which
            // nothing else refers to; its entries are plain words.
            drop(unsafe { Vec::from_raw_parts(self.spill, 0, self.capacity) });
            self.spill = ptr::null_mut();
            self.entries = self.inline.as_mut_ptr();
            self.capacity = DIRECT_FRAME_STACK_INLINE_ENTRIES;
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }

    #[cfg(test)]
    pub(super) fn limit(&self) -> usize {
        self.limit
    }

    #[cfg(test)]
    pub(super) fn flags(&self) -> u64 {
        self.flags
    }

    #[cfg(test)]
    pub(super) fn is_spilled(&self) -> bool {
        !self.spill.is_null()
    }

    /// Pushes `descriptor` the way generated code does, when its rule
    /// allows. Returns whether it did.
    #[cfg(test)]
    pub(super) unsafe fn generated_push(
        stack: *mut Self,
        descriptor: *const DirectFrameDescriptor,
    ) -> bool {
        let layout = direct_frame_stack_layout();
        let base = stack.cast::<u8>();
        // SAFETY: the caller passes a live stack and descriptor; these are
        // the reads and writes generated code makes.
        unsafe {
            let read = |offset: i32| base.offset(offset as isize).cast::<usize>().read();
            let len = read(layout.len_offset);
            if len >= read(layout.limit_offset) {
                return false;
            }
            let entries = read(layout.entries_offset) as *mut usize;
            entries.add(len).write(descriptor as usize);
            base.offset(layout.len_offset as isize)
                .cast::<usize>()
                .write(len + 1);
        }
        true
    }

    /// Pops a call the way generated code does, when its rule allows.
    /// Returns whether it did.
    #[cfg(test)]
    pub(super) unsafe fn generated_pop(stack: *mut Self) -> bool {
        let layout = direct_frame_stack_layout();
        let base = stack.cast::<u8>();
        // SAFETY: the caller passes a live stack.
        unsafe {
            let read = |offset: i32| base.offset(offset as isize).cast::<usize>().read();
            let len = read(layout.len_offset);
            if len <= read(layout.floor_offset) {
                return false;
            }
            base.offset(layout.len_offset as isize)
                .cast::<usize>()
                .write(len - 1);
        }
        true
    }
}

/// The frame record one entry describes.
fn frame_from_word(word: usize) -> DirectRuntimeCallFrame {
    if word & DIRECT_OWNED_FRAME_TAG != 0 {
        let frame = (word & !DIRECT_OWNED_FRAME_TAG) as *const DirectRuntimeCallFrame;
        // SAFETY: the stack owns the record until it pops the entry.
        unsafe { &*frame }.clone()
    } else {
        // SAFETY: descriptors are immutable program data that outlive every
        // call, and the descriptor was validated before it was pushed.
        unsafe { &*(word as *const DirectFrameDescriptor) }.frame()
    }
}

fn release_frame_word(word: usize) {
    if word & DIRECT_OWNED_FRAME_TAG != 0 {
        let frame = (word & !DIRECT_OWNED_FRAME_TAG) as *mut DirectRuntimeCallFrame;
        // SAFETY: the entry owned the record, and it was just removed.
        drop(unsafe { Box::from_raw(frame) });
    }
}

/// Where generated code finds a frame stack's header. The offsets hold for
/// one build of the runtime, so the
/// compiler records [`DirectFrameStackLayout::fingerprint`] in each
/// executable and `aura_direct_check_frame_stack_layout` compares it at
/// startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectFrameStackLayout {
    /// The offset of the entry count.
    pub len_offset: i32,
    /// The offset of the count below which generated code may push.
    pub limit_offset: i32,
    /// The offset of the count above which generated code may pop.
    pub floor_offset: i32,
    /// The offset of the address of the first entry.
    pub entries_offset: i32,
}

impl DirectFrameStackLayout {
    /// The bytes of one entry.
    pub const ENTRY_BYTES: i64 = std::mem::size_of::<usize>() as i64;

    /// One number that differs whenever an offset or a rule differs.
    pub fn fingerprint(&self) -> i64 {
        let fields = [
            i64::from(self.len_offset),
            i64::from(self.limit_offset),
            i64::from(self.floor_offset),
            i64::from(self.entries_offset),
            Self::ENTRY_BYTES,
        ];
        fields.iter().fold(0x46_72_61_6d_i64, |hash, field| {
            hash.wrapping_mul(0x100_0000_01b3).wrapping_add(*field)
        })
    }
}

/// The layout of this build of the runtime.
pub fn direct_frame_stack_layout() -> DirectFrameStackLayout {
    DirectFrameStackLayout {
        len_offset: std::mem::offset_of!(DirectFrameStack, len) as i32,
        limit_offset: std::mem::offset_of!(DirectFrameStack, limit) as i32,
        floor_offset: std::mem::offset_of!(DirectFrameStack, pop_floor) as i32,
        entries_offset: std::mem::offset_of!(DirectFrameStack, entries) as i32,
    }
}

#[cfg(test)]
#[path = "native_frame_stack_tests.rs"]
mod tests;
