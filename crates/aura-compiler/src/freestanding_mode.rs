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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FreestandingTarget::{Aarch64, X86_64};

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
