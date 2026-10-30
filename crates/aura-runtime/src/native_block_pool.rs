//! A small cache of freed layout blocks for each thread.
//!
//! An enum variant or a class instance kept in its typed layout holds its
//! payloads or fields in a heap block of its own, and a program that builds
//! many small variants, such as the nodes of a tree, makes and frees one
//! block for each. A block of at most `MAX_SIZE` bytes freed here goes back
//! on this thread's list for its size, and the next block of that size
//! this thread makes takes it from there without calling the allocator.
//! Each list is bounded, so a burst of frees hands the rest back.

use std::alloc::{alloc, alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::cell::Cell;
use std::ptr::{self, NonNull};

/// The size step of the lists: every block is rounded up to it.
const STEP: usize = 16;
/// The largest block the lists keep.
const MAX_SIZE: usize = 128;
const CLASSES: usize = MAX_SIZE / STEP;
/// The most blocks one thread keeps of each size.
const CAPACITY: usize = 1024;

/// A cached block, linked through its first word.
struct FreeBlock {
    next: *mut FreeBlock,
}

struct BlockList {
    head: Cell<*mut FreeBlock>,
    len: Cell<usize>,
}

struct BlockPool {
    lists: [BlockList; CLASSES],
}

/// The list for blocks of `layout`, by index, when the pool keeps them.
fn class_of(layout: Layout) -> Option<usize> {
    (layout.size() <= MAX_SIZE && layout.align() <= STEP)
        .then(|| layout.size().max(1).div_ceil(STEP) - 1)
}

/// The layout every block of list `class` is allocated with.
fn class_layout(class: usize) -> Layout {
    Layout::from_size_align((class + 1) * STEP, STEP).expect("a small block layout")
}

impl BlockPool {
    fn pop(&self, class: usize) -> Option<*mut u8> {
        let list = &self.lists[class];
        let head = list.head.get();
        if head.is_null() {
            return None;
        }
        // SAFETY: a cached block is ours until it leaves the list.
        list.head.set(unsafe { (*head).next });
        list.len.set(list.len.get() - 1);
        Some(head.cast())
    }

    fn push(&self, class: usize, block: *mut u8) -> bool {
        let list = &self.lists[class];
        if list.len.get() == CAPACITY {
            return false;
        }
        let free = block.cast::<FreeBlock>();
        // SAFETY: the caller hands over a block it no longer uses, at least
        // `STEP` bytes long and aligned to it.
        unsafe {
            free.write(FreeBlock {
                next: list.head.get(),
            })
        };
        list.head.set(free);
        list.len.set(list.len.get() + 1);
        true
    }
}

impl Drop for BlockPool {
    fn drop(&mut self) {
        for class in 0..CLASSES {
            while let Some(block) = self.pop(class) {
                // SAFETY: every cached block of the list came from
                // `allocate` with the list's layout.
                unsafe { dealloc(block, class_layout(class)) };
            }
        }
    }
}

thread_local! {
    static BLOCK_POOL: BlockPool = const {
        BlockPool {
            lists: [const {
                BlockList {
                    head: Cell::new(ptr::null_mut()),
                    len: Cell::new(0),
                }
            }; CLASSES],
        }
    };
}

/// A block for `layout`, zeroed when `zeroed` is set, reusing one this
/// thread cached when it can. Free it with [`free`] and the same layout.
pub(super) fn allocate(layout: Layout, zeroed: bool) -> NonNull<u8> {
    let Some(class) = class_of(layout) else {
        // SAFETY: a layout block is at least one byte.
        let block = unsafe {
            if zeroed {
                alloc_zeroed(layout)
            } else {
                alloc(layout)
            }
        };
        return NonNull::new(block).unwrap_or_else(|| handle_alloc_error(layout));
    };
    let block = match BLOCK_POOL.try_with(|pool| pool.pop(class)).ok().flatten() {
        Some(block) => {
            if zeroed {
                // SAFETY: the cached block is at least `layout.size()` bytes.
                unsafe { ptr::write_bytes(block, 0, layout.size()) };
            }
            block
        }
        // SAFETY: as above.
        None => unsafe {
            let class = class_layout(class);
            if zeroed {
                alloc_zeroed(class)
            } else {
                alloc(class)
            }
        },
    };
    NonNull::new(block).unwrap_or_else(|| handle_alloc_error(layout))
}

/// Caches `block` for this thread, or frees it.
///
/// # Safety
///
/// `block` came from [`allocate`] with `layout`, and nothing uses it again.
pub(super) unsafe fn free(block: NonNull<u8>, layout: Layout) {
    let block = block.as_ptr();
    let Some(class) = class_of(layout) else {
        // SAFETY: the caller's contract.
        unsafe { dealloc(block, layout) };
        return;
    };
    if !BLOCK_POOL
        .try_with(|pool| pool.push(class, block))
        .unwrap_or(false)
    {
        // SAFETY: the block came from `allocate` with the class's layout.
        unsafe { dealloc(block, class_layout(class)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freed_block_is_reused_for_the_same_size_until_the_list_is_full() {
        let layout = Layout::from_size_align(24, 8).unwrap();
        let first = allocate(layout, true);
        // SAFETY: the test holds the block.
        unsafe { free(first, layout) };
        let second = allocate(Layout::from_size_align(32, 16).unwrap(), true);
        assert_eq!(first, second, "a block of the same size class is reused");
        // SAFETY: the reused block is zeroed.
        assert!(unsafe { std::slice::from_raw_parts(second.as_ptr(), 32) }
            .iter()
            .all(|byte| *byte == 0));
        // SAFETY: as above.
        unsafe { free(second, Layout::from_size_align(32, 16).unwrap()) };

        let blocks = (0..CAPACITY + 1)
            .map(|_| allocate(layout, false))
            .collect::<Vec<_>>();
        for block in blocks {
            // SAFETY: as above.
            unsafe { free(block, layout) };
        }
        BLOCK_POOL.with(|pool| assert_eq!(pool.lists[1].len.get(), CAPACITY));

        // Large or strictly aligned blocks go to the allocator.
        for layout in [
            Layout::from_size_align(MAX_SIZE + 1, 8).unwrap(),
            Layout::from_size_align(16, 32).unwrap(),
        ] {
            let block = allocate(layout, true);
            // SAFETY: as above.
            unsafe { free(block, layout) };
        }
    }
}
