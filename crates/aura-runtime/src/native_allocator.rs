//! The heap allocator of every program, and of the tools that link the
//! runtime.
//!
//! A program makes and frees small blocks all the time: list and string
//! buffers, dictionary tables, boxes. The system allocator searches its
//! bins for most of them, which costs a program that builds and drops many
//! small values far more than its own work. This allocator keeps each
//! thread's freed blocks of each size up to `MAX_SIZE` bytes on a list of
//! its own, and makes new ones by cutting them from a chunk the thread
//! holds, so a small block costs a few instructions either way.
//!
//! Small blocks never go back to the system allocator. A thread whose list
//! of a size is full hands half of it to a shared list for that size, which
//! a thread with an empty list takes from before it cuts a new block, and a
//! thread that ends hands over all its lists and the rest of its chunk. So
//! the small blocks a program holds never grow past the most it had in use
//! at once, whichever threads make and free them. Larger blocks, and blocks
//! aligned beyond `STEP`, go to the system allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

/// The size step of the lists: every small block is rounded up to it.
const STEP: usize = 16;
/// The largest block the lists keep.
const MAX_SIZE: usize = 1024;
const CLASSES: usize = MAX_SIZE / STEP;
/// The most blocks one thread keeps of each size.
const CAPACITY: u32 = 1024;
/// The size of the chunks threads cut new small blocks from.
const CHUNK: usize = 64 * 1024;

/// The allocator `aura build` executables and the Aura tools run on.
pub struct AuraAllocator;

/// A free block, linked through its first word. The first block of a batch
/// on a shared list also links the next batch through its second word.
struct FreeBlock {
    next: *mut FreeBlock,
    next_batch: *mut FreeBlock,
}

struct BlockList {
    head: Cell<*mut FreeBlock>,
    len: Cell<u32>,
}

struct ThreadBlocks {
    lists: [BlockList; CLASSES],
    /// The uncut part of the thread's chunk, from `cut` to `end`.
    cut: Cell<*mut u8>,
    end: Cell<*mut u8>,
}

/// The batches of free blocks of one size that no thread holds, behind a
/// spin lock: a lock that could allocate cannot guard the allocator.
struct SharedList {
    locked: AtomicBool,
    /// Changed only under the lock, and read without it only to see that
    /// the list is empty.
    head: AtomicPtr<FreeBlock>,
}

static SHARED: [SharedList; CLASSES] = [const {
    SharedList {
        locked: AtomicBool::new(false),
        head: AtomicPtr::new(ptr::null_mut()),
    }
}; CLASSES];

impl SharedList {
    fn with<T>(&self, work: impl FnOnce(&AtomicPtr<FreeBlock>) -> T) -> T {
        let mut spins = 0u32;
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spins += 1;
            if spins < 64 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
        let result = work(&self.head);
        self.locked.store(false, Ordering::Release);
        result
    }
}

/// The list for blocks of `layout`, by index, when the lists keep them.
#[inline]
fn class_of(layout: Layout) -> Option<usize> {
    (layout.size() <= MAX_SIZE && layout.align() <= STEP)
        .then(|| layout.size().max(1).div_ceil(STEP) - 1)
}

#[inline]
fn class_size(class: usize) -> usize {
    (class + 1) * STEP
}

/// The last block of the batch that starts at `head`.
///
/// # Safety
///
/// `head` starts a batch of linked free blocks that ends with a null link.
unsafe fn batch_tail(head: *mut FreeBlock) -> *mut FreeBlock {
    let mut tail = head;
    // SAFETY: the caller's contract.
    unsafe {
        while !(*tail).next.is_null() {
            tail = (*tail).next;
        }
    }
    tail
}

/// Puts the batch `head`..`tail` on the shared list of `class`.
///
/// # Safety
///
/// The blocks of the batch are free blocks of the class that no one else
/// holds, linked from `head` to `tail`.
unsafe fn share_batch(class: usize, head: *mut FreeBlock, tail: *mut FreeBlock) {
    SHARED[class].with(|shared| {
        // SAFETY: the caller hands the batch over.
        unsafe {
            (*tail).next = ptr::null_mut();
            (*head).next_batch = shared.load(Ordering::Relaxed);
        }
        shared.store(head, Ordering::Relaxed);
    });
}

/// The first batch of the shared list of `class`, or null.
fn take_shared_batch(class: usize) -> *mut FreeBlock {
    let list = &SHARED[class];
    if list.head.load(Ordering::Relaxed).is_null() {
        return ptr::null_mut();
    }
    list.with(|shared| {
        let head = shared.load(Ordering::Relaxed);
        if !head.is_null() {
            // SAFETY: the batch is on the list, so no one else holds it.
            shared.store(unsafe { (*head).next_batch }, Ordering::Relaxed);
        }
        head
    })
}

impl ThreadBlocks {
    #[inline]
    fn pop(&self, class: usize) -> *mut u8 {
        let list = &self.lists[class];
        let head = list.head.get();
        if head.is_null() {
            return self.refill(class);
        }
        // SAFETY: a free block on the list is ours until it leaves it.
        list.head.set(unsafe { (*head).next });
        list.len.set(list.len.get() - 1);
        head.cast()
    }

    /// A block of `class` for a thread whose list of it is empty: the first
    /// of a shared batch, whose rest becomes the list, or one cut from the
    /// thread's chunk.
    #[cold]
    fn refill(&self, class: usize) -> *mut u8 {
        let batch = take_shared_batch(class);
        if !batch.is_null() {
            // SAFETY: the batch is ours now, and ends with a null link.
            let rest = unsafe { (*batch).next };
            let mut len = 0;
            let mut block = rest;
            while !block.is_null() {
                len += 1;
                // SAFETY: as above.
                block = unsafe { (*block).next };
            }
            let list = &self.lists[class];
            list.head.set(rest);
            list.len.set(len);
            return batch.cast();
        }
        let size = class_size(class);
        let mut cut = self.cut.get();
        if (self.end.get() as usize) - (cut as usize) < size {
            self.share_rest_of_chunk();
            // SAFETY: the chunk layout has a nonzero size.
            cut = unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK, STEP)) };
            if cut.is_null() {
                return cut;
            }
            // SAFETY: the end is one past the new chunk.
            self.end.set(unsafe { cut.add(CHUNK) });
        }
        // SAFETY: at least `size` bytes remain before the end of the chunk.
        self.cut.set(unsafe { cut.add(size) });
        cut
    }

    #[inline]
    fn push(&self, class: usize, block: *mut u8) {
        let list = &self.lists[class];
        if list.len.get() == CAPACITY {
            self.share_half(class);
        }
        let free = block.cast::<FreeBlock>();
        // SAFETY: the caller hands over a block it no longer uses, at least
        // `STEP` bytes long and aligned to it.
        unsafe { (*free).next = list.head.get() };
        list.head.set(free);
        list.len.set(list.len.get() + 1);
    }

    /// Hands the first half of the full list of `class` to its shared list.
    #[cold]
    fn share_half(&self, class: usize) {
        let list = &self.lists[class];
        let head = list.head.get();
        let mut tail = head;
        for _ in 1..CAPACITY / 2 {
            // SAFETY: the full list holds `CAPACITY` linked blocks.
            tail = unsafe { (*tail).next };
        }
        // SAFETY: as above, so the tail has a successor.
        list.head.set(unsafe { (*tail).next });
        list.len.set(CAPACITY - CAPACITY / 2);
        // SAFETY: the batch left the thread's list.
        unsafe { share_batch(class, head, tail) };
    }

    /// Cuts the uncut part of the thread's chunk into blocks of the largest
    /// sizes that fit, and hands them to the shared lists.
    fn share_rest_of_chunk(&self) {
        let mut cut = self.cut.get();
        let end = self.end.get();
        while (end as usize) - (cut as usize) >= STEP {
            let class = (((end as usize) - (cut as usize)) / STEP).min(CLASSES) - 1;
            let block = cut.cast::<FreeBlock>();
            // SAFETY: the block lies in the uncut part, which no one holds.
            unsafe {
                share_batch(class, block, block);
                cut = cut.add(class_size(class));
            }
        }
        self.cut.set(end);
    }
}

impl Drop for ThreadBlocks {
    fn drop(&mut self) {
        for (class, list) in self.lists.iter().enumerate() {
            let head = list.head.replace(ptr::null_mut());
            list.len.set(0);
            if !head.is_null() {
                // SAFETY: the blocks left the thread's list, which ends with
                // a null link.
                unsafe { share_batch(class, head, batch_tail(head)) };
            }
        }
        self.share_rest_of_chunk();
    }
}

thread_local! {
    static THREAD_BLOCKS: ThreadBlocks = const {
        ThreadBlocks {
            lists: [const {
                BlockList {
                    head: Cell::new(ptr::null_mut()),
                    len: Cell::new(0),
                }
            }; CLASSES],
            cut: Cell::new(ptr::null_mut()),
            end: Cell::new(ptr::null_mut()),
        }
    };
}

/// A block of `class`: the thread's, or once the thread's lists are gone, a
/// shared one or a new system block.
#[inline]
fn small_block(class: usize) -> *mut u8 {
    THREAD_BLOCKS
        .try_with(|blocks| blocks.pop(class))
        .unwrap_or_else(|_| unlisted_block(class))
}

#[cold]
fn unlisted_block(class: usize) -> *mut u8 {
    let batch = take_shared_batch(class);
    if batch.is_null() {
        // SAFETY: a class layout has a nonzero size.
        return unsafe { System.alloc(Layout::from_size_align_unchecked(class_size(class), STEP)) };
    }
    // SAFETY: the batch is ours now; the rest of it goes back.
    unsafe {
        let rest = (*batch).next;
        if !rest.is_null() {
            share_batch(class, rest, batch_tail(rest));
        }
    }
    batch.cast()
}

// SAFETY: every block handed out is at least as large and as aligned as its
// layout asks, and each small block is either in use or on exactly one
// list.
unsafe impl GlobalAlloc for AuraAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match class_of(layout) {
            Some(class) => small_block(class),
            // SAFETY: the caller's contract.
            None => unsafe { System.alloc(layout) },
        }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let Some(class) = class_of(layout) else {
            // SAFETY: the caller's contract.
            return unsafe { System.alloc_zeroed(layout) };
        };
        let block = small_block(class);
        if !block.is_null() {
            // SAFETY: the block is at least `layout.size()` bytes.
            unsafe { ptr::write_bytes(block, 0, layout.size()) };
        }
        block
    }

    #[inline]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        let Some(class) = class_of(layout) else {
            // SAFETY: the caller's contract.
            return unsafe { System.dealloc(block, layout) };
        };
        if THREAD_BLOCKS
            .try_with(|blocks| blocks.push(class, block))
            .is_err()
        {
            let free = block.cast::<FreeBlock>();
            // SAFETY: the caller hands the block over.
            unsafe { share_batch(class, free, free) };
        }
    }

    #[inline]
    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's contract makes `new_size` a valid size for
        // the alignment.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (class_of(layout), class_of(new_layout)) {
            // SAFETY: the caller's contract.
            (None, None) => unsafe { System.realloc(block, layout, new_size) },
            (Some(old), Some(new)) if old == new => block,
            _ => {
                // SAFETY: the caller's contract, for a new block and the old.
                unsafe {
                    let moved = self.alloc(new_layout);
                    if !moved.is_null() {
                        ptr::copy_nonoverlapping(block, moved, layout.size().min(new_size));
                        self.dealloc(block, layout);
                    }
                    moved
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "native_allocator_tests.rs"]
mod tests;
