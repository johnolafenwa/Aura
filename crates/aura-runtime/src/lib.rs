//! The Aura native runtime.
//!
//! Executables built by `aura build` link this crate's static library. It
//! holds runtime values, the scheduler and reactor, host builtins, the JSON and
//! byte codecs, and the FFI bridge. It depends on `aura-core` and never on the
//! compiler, so nothing compiler-only can reach a user executable.

pub use aura_core::{diag, integer};

mod array_elementwise;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod array_kernel_corpus;
pub mod array_layout;
mod array_members;
mod array_reduce;
pub mod array_simd;
pub mod array_views;
pub mod bytes_codec;
pub mod c_layout;
pub mod collection_index;
pub mod ffi;
pub mod gzip_codec;
pub mod host_io;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod host_tools;
pub mod json_codec;
pub mod json_tape;
pub mod list_elements;
pub mod list_sort;
mod native_allocator;
pub mod native_runtime;
#[cfg(any(windows, test))]
mod pipe_bridge;
pub mod randomness;
pub mod runtime_config;
pub mod runtime_reactor;
pub mod runtime_value;
#[cfg(any(windows, test))]
mod socket_watcher;
pub mod text;
pub mod text_methods;
pub mod union_runtime;
#[cfg(windows)]
mod windows_process;

#[global_allocator]
static ALLOCATOR: native_allocator::AuraAllocator = native_allocator::AuraAllocator;

#[doc(hidden)]
pub const INTERNAL_DIAGNOSTIC_FD_ENV: &str = "AURA_INTERNAL_DIAGNOSTIC_FD";
#[doc(hidden)]
pub const INTERNAL_DIAGNOSTIC_SIGNAL_FD_ENV: &str = "AURA_INTERNAL_DIAGNOSTIC_SIGNAL_FD";
#[doc(hidden)]
pub const INTERNAL_DIAGNOSTIC_SIGNAL_MARKER: u8 = 0x01;
#[doc(hidden)]
pub const MAX_INTERNAL_DIAGNOSTIC_BYTES: usize = 1024 * 1024;

#[cfg(test)]
pub(crate) fn hosted_ci_timing_limit(local_limit: std::time::Duration) -> std::time::Duration {
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        local_limit.saturating_mul(4)
    } else {
        local_limit
    }
}
