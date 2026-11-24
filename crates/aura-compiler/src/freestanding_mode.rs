//! The target of a freestanding source check. Library calls outside a
//! freestanding request use the hosted language rules.
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::FreestandingTarget;

thread_local! {
    static TARGET: Cell<Option<FreestandingTarget>> = const { Cell::new(None) };
}

/// The freestanding target of this thread's current compiler request.
pub fn freestanding_check_target() -> Option<FreestandingTarget> {
    TARGET.get()
}

/// Restores the enclosing check's target on drop, including during unwind.
#[must_use]
pub struct FreestandingScope {
    previous: Option<FreestandingTarget>,
    // The guard restores thread-local state and must stay on this thread.
    thread: PhantomData<Rc<()>>,
}

impl Drop for FreestandingScope {
    fn drop(&mut self) {
        TARGET.set(self.previous);
    }
}

/// Selects a target until the returned guard leaves scope.
pub fn enter_freestanding_target(target: FreestandingTarget) -> FreestandingScope {
    FreestandingScope {
        previous: TARGET.replace(Some(target)),
        thread: PhantomData,
    }
}

/// Runs one synchronous compiler request for a freestanding target.
pub fn with_freestanding_target<T>(target: FreestandingTarget, run: impl FnOnce() -> T) -> T {
    let _scope = enter_freestanding_target(target);
    run()
}

/// Library calls LLVM emits for Aura's float32 and float64 operations with
/// the x86-64 soft-float ABI. The kernel supplies these at link time.
pub(crate) fn is_soft_float_helper(name: &str) -> bool {
    freestanding_check_target() == Some(FreestandingTarget::X86_64Kernel)
        && [
            "__addsf3",
            "__adddf3",
            "__subsf3",
            "__subdf3",
            "__mulsf3",
            "__muldf3",
            "__divsf3",
            "__divdf3",
            "__eqsf2",
            "__eqdf2",
            "__nesf2",
            "__nedf2",
            "__ltsf2",
            "__ltdf2",
            "__lesf2",
            "__ledf2",
            "__gtsf2",
            "__gtdf2",
            "__gesf2",
            "__gedf2",
            "__unordsf2",
            "__unorddf2",
            "__extendsfdf2",
            "__truncdfsf2",
            "__fixsfsi",
            "__fixsfdi",
            "__fixdfsi",
            "__fixdfdi",
            "__fixunssfsi",
            "__fixunssfdi",
            "__fixunsdfsi",
            "__fixunsdfdi",
            "__floatsisf",
            "__floatsidf",
            "__floatdisf",
            "__floatdidf",
            "__floatunsisf",
            "__floatunsidf",
            "__floatundisf",
            "__floatundidf",
            "sqrtf",
            "sqrt",
        ]
        .contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FreestandingTarget::{Aarch64, X86_64};

    #[test]
    fn only_kernel_objects_may_reference_soft_float_helpers() {
        assert!(!is_soft_float_helper("__adddf3"));
        with_freestanding_target(crate::FreestandingTarget::X86_64Kernel, || {
            assert!(is_soft_float_helper("__adddf3"));
            assert!(is_soft_float_helper("sqrt"));
            assert!(!is_soft_float_helper("__arbitrary_helper"));
        });
    }

    #[test]
    fn a_target_is_scoped_to_its_request_and_thread() {
        assert_eq!(freestanding_check_target(), None);
        with_freestanding_target(Aarch64, || {
            assert_eq!(freestanding_check_target(), Some(Aarch64));
            std::thread::spawn(|| assert_eq!(freestanding_check_target(), None))
                .join()
                .unwrap();
            let _inner = enter_freestanding_target(X86_64);
            assert_eq!(freestanding_check_target(), Some(X86_64));
        });
        assert_eq!(freestanding_check_target(), None);
    }
}
