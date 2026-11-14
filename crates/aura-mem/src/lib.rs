//! `memcpy`, `memmove`, `memcmp` and `bcmp` for programs linked against
//! musl (ADR-0101).
//!
//! musl's x86-64 `memcpy` and `memmove` are `rep movsq` for every size,
//! which costs tens of cycles to start, and its `memcmp` compares a byte
//! at a time on every architecture. Aura programs copy and compare short
//! runs of bytes constantly, and a musl build took up to 1.8 times as long
//! as the glibc build of the same program.
//!
//! On a musl target this crate defines the C symbols, and the runtime
//! library comes before the C library in the link, so the linker never
//! takes musl's. On arm64, musl's `memcpy` and `memmove` are already
//! tuned assembly, so only the comparisons are defined there. On every
//! other target the crate defines no symbol and the routines are reached
//! by their Rust names, which is how the tests run them.
//!
//! The crate is `no_builtins`: the compiler must not turn a copy loop
//! here into a call to `memcpy`, which would be a call to itself.

#![no_std]
#![no_builtins]

use core::ptr::{read_unaligned, write_unaligned};

/// From this many bytes a forward copy on x86-64 is one `rep movsb`,
/// which the processor runs a cache line at a time.
#[cfg(target_arch = "x86_64")]
const STRING_COPY_FROM: usize = 2048;

#[inline(always)]
unsafe fn load<T>(source: *const u8, offset: usize) -> T {
    // SAFETY: the caller keeps `offset + size_of::<T>()` within the run.
    unsafe { read_unaligned(source.add(offset).cast::<T>()) }
}

#[inline(always)]
unsafe fn store<T>(destination: *mut u8, offset: usize, value: T) {
    // SAFETY: the caller keeps `offset + size_of::<T>()` within the run.
    unsafe { write_unaligned(destination.add(offset).cast::<T>(), value) }
}

/// Copies up to 32 bytes. Every byte is read before any is written, so
/// the runs may overlap.
#[inline(always)]
unsafe fn copy_short(destination: *mut u8, source: *const u8, count: usize) {
    // SAFETY: each branch reads and writes two blocks that lie within
    // `count` bytes: one at the start and one that ends at the end.
    unsafe {
        if count >= 16 {
            let head: u128 = load(source, 0);
            let tail: u128 = load(source, count - 16);
            store(destination, 0, head);
            store(destination, count - 16, tail);
        } else if count >= 8 {
            let head: u64 = load(source, 0);
            let tail: u64 = load(source, count - 8);
            store(destination, 0, head);
            store(destination, count - 8, tail);
        } else if count >= 4 {
            let head: u32 = load(source, 0);
            let tail: u32 = load(source, count - 4);
            store(destination, 0, head);
            store(destination, count - 4, tail);
        } else if count >= 2 {
            let head: u16 = load(source, 0);
            let tail: u16 = load(source, count - 2);
            store(destination, 0, head);
            store(destination, count - 2, tail);
        } else if count == 1 {
            *destination = *source;
        }
    }
}

/// Copies more than 32 bytes from the first byte up. The destination may
/// overlap the source only from below.
#[inline(always)]
unsafe fn copy_forward(destination: *mut u8, source: *const u8, count: usize) {
    // SAFETY: the loop copies whole 32-byte blocks that end at or before
    // `count - 32`, and the last block, read before the loop writes
    // anything, covers the rest. A block is read whole before it is
    // written, and the destination is not above the source, so no write
    // lands on a byte still to be read.
    unsafe {
        let tail: [u128; 2] = load(source, count - 32);
        let mut offset = 0;
        while offset < count - 32 {
            let block: [u128; 2] = load(source, offset);
            store(destination, offset, block);
            offset += 32;
        }
        store(destination, count - 32, tail);
    }
}

/// Copies more than 32 bytes from the last byte down. The destination
/// may overlap the source only from above.
#[inline(always)]
unsafe fn copy_backward(destination: *mut u8, source: *const u8, count: usize) {
    // SAFETY: the mirror of `copy_forward`: whole blocks from the end
    // down to offset 32 or above, and the first block, read before the
    // loop writes anything, covers the rest.
    unsafe {
        let head: [u128; 2] = load(source, 0);
        let mut end = count;
        while end > 32 {
            let block: [u128; 2] = load(source, end - 32);
            store(destination, end - 32, block);
            end -= 32;
        }
        store(destination, 0, head);
    }
}

/// Copies `count` bytes between runs that do not overlap.
///
/// # Safety
///
/// `source` is readable and `destination` writable for `count` bytes, and
/// the two runs do not overlap.
#[inline]
pub unsafe fn copy(destination: *mut u8, source: *const u8, count: usize) {
    // SAFETY: the caller's contract is each callee's.
    unsafe {
        if count <= 32 {
            return copy_short(destination, source, count);
        }
        #[cfg(target_arch = "x86_64")]
        if count >= STRING_COPY_FROM {
            // The direction flag is clear on entry to every function.
            core::arch::asm!(
                "rep movsb",
                inout("rcx") count => _,
                inout("rdi") destination => _,
                inout("rsi") source => _,
                options(nostack, preserves_flags)
            );
            return;
        }
        copy_forward(destination, source, count);
    }
}

/// Copies `count` bytes between runs that may overlap.
///
/// # Safety
///
/// `source` is readable and `destination` writable for `count` bytes.
#[inline]
pub unsafe fn copy_overlapping(destination: *mut u8, source: *const u8, count: usize) {
    // SAFETY: the caller's contract is each callee's, and the direction
    // is the one that never writes a byte still to be read.
    unsafe {
        if count <= 32 {
            copy_short(destination, source, count);
        } else if (destination as usize).wrapping_sub(source as usize) >= count {
            // The destination starts below the source or past its end.
            copy_forward(destination, source, count);
        } else {
            copy_backward(destination, source, count);
        }
    }
}

/// Compares `count` bytes as `memcmp` does: negative, zero or positive as
/// the first run sorts before, equal to or after the second.
///
/// # Safety
///
/// Both runs are readable for `count` bytes.
#[inline]
pub unsafe fn compare(left: *const u8, right: *const u8, count: usize) -> i32 {
    #[inline(always)]
    fn order(left: u64, right: u64) -> i32 {
        // The first byte in memory decides, which is the top byte of the
        // big-endian reading.
        if u64::from_be(left) < u64::from_be(right) {
            -1
        } else {
            1
        }
    }
    // SAFETY: each load lies within `count` bytes.
    unsafe {
        let mut offset = 0;
        while offset + 8 <= count {
            let (a, b): (u64, u64) = (load(left, offset), load(right, offset));
            if a != b {
                return order(a, b);
            }
            offset += 8;
        }
        if offset == count {
            return 0;
        }
        if count >= 8 {
            // The last eight bytes overlap ones already found equal.
            let (a, b): (u64, u64) = (load(left, count - 8), load(right, count - 8));
            return if a == b { 0 } else { order(a, b) };
        }
        while offset < count {
            let (a, b) = (*left.add(offset), *right.add(offset));
            if a != b {
                return i32::from(a) - i32::from(b);
            }
            offset += 1;
        }
        0
    }
}

#[cfg(all(target_env = "musl", target_arch = "x86_64"))]
mod copies {
    /// # Safety
    ///
    /// The C contract of `memcpy`.
    #[no_mangle]
    pub unsafe extern "C" fn memcpy(
        destination: *mut u8,
        source: *const u8,
        count: usize,
    ) -> *mut u8 {
        // SAFETY: the C contract is `copy`'s.
        unsafe { super::copy(destination, source, count) };
        destination
    }

    /// # Safety
    ///
    /// The C contract of `memmove`.
    #[no_mangle]
    pub unsafe extern "C" fn memmove(
        destination: *mut u8,
        source: *const u8,
        count: usize,
    ) -> *mut u8 {
        // SAFETY: the C contract is `copy_overlapping`'s.
        unsafe { super::copy_overlapping(destination, source, count) };
        destination
    }
}

#[cfg(target_env = "musl")]
mod comparisons {
    /// # Safety
    ///
    /// The C contract of `memcmp`.
    #[no_mangle]
    pub unsafe extern "C" fn memcmp(left: *const u8, right: *const u8, count: usize) -> i32 {
        // SAFETY: the C contract is `compare`'s.
        unsafe { super::compare(left, right, count) }
    }

    /// # Safety
    ///
    /// The C contract of `bcmp`.
    #[no_mangle]
    pub unsafe extern "C" fn bcmp(left: *const u8, right: *const u8, count: usize) -> i32 {
        // SAFETY: the C contract is `compare`'s.
        unsafe { super::compare(left, right, count) }
    }
}

#[cfg(test)]
mod mem_tests;
