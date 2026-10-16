//! Task limits shared by the checker and the runtime.

/// The smallest task stack a program may request, in bytes.
pub const MIN_TASK_STACK_BYTES: i64 = 256 * 1024;
/// The largest task stack a program may request, in bytes.
pub const MAX_TASK_STACK_BYTES: i64 = 64 * 1024 * 1024;
