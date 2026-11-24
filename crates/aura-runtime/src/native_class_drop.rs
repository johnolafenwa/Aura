//! Direct ABI for arming and invoking an instance's owner hook. A hook
//! runs outside Rust drop glue, using the cleanup trap boundary.

use super::*;
static CLASS_HOOKS_ARMED: AtomicBool = AtomicBool::new(false);

use crate::runtime_value::class_drop::{released_instances, DropMethod};

/// Registers the compiler's generated entry for a fully initialized receiver.
///
/// # Safety
/// The caller holds exclusive access to an instance handle and supplies
/// its generated cleanup thunk with the native thunk ABI.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_arm_class_drop(
    value: *mut OpaqueValue,
    thunk: NativeThunk,
) {
    task_runtime_boundary(|| unsafe {
        CLASS_HOOKS_ARMED.store(true, Ordering::Relaxed);
        value_mut(value, |value| {
            let Value::Instance(instance) = value else {
                runtime_error("invalid drop hook receiver")
            };
            instance.arm_drop(DropMethod::Native(thunk));
        });
    })
}

#[cfg_attr(not(coverage), no_mangle)]
pub extern "C-unwind" fn aura_direct_drain_class_drops() {
    if let Some(error) = drain_released_hooks() {
        leaf_trap(|| error);
    }
}

fn drain_released_hooks() -> Option<Diagnostic> {
    let mut first = None;
    for (method, value) in released_instances() {
        let DropMethod::Native(thunk) = method else {
            runtime_error("interpreted drop hook inside native code")
        };
        let failure = call_hook(thunk, value);
        let fields = drain_released_hooks();
        first = first.or(failure).or(fields);
    }
    first
}

/// Each hook gets a cleanup stack and ownership ledger of its own. A trap
/// can abandon temporaries in the hook without releasing its caller's
/// temporaries or unwinding through a Rust destructor.
fn call_hook(thunk: NativeThunk, value: InstanceValue) -> Option<Diagnostic> {
    let key = direct_task_runtime_key();
    let ancestry = direct_runtime_compact_task_ancestry();
    let mut state = boxed_direct_task_runtime_state(true, ancestry);
    state.cleanup_draining = true;
    let scope = install_direct_task_runtime_state(state);
    let receiver = boxed_value(Value::Instance(value));
    let mut arg = receiver as i64;
    // SAFETY: the compiler supplies the thunk for this initialized receiver.
    unsafe {
        run_drained_cleanup_thunk(key, thunk, &mut arg, 1);
        aura_direct_release_value(receiver);
    }
    with_direct_task_runtime_state(|state| state.cleanup_draining = false);
    drain_direct_cleanup_registrations(key);
    let failure = with_direct_task_runtime_state(|state| state.primary_runtime_diagnostic.take());
    // The caller retains the active diagnostic. A trapping hook may have
    // left references in this ledger and those fields have their own hooks.
    if failure.is_some() {
        release_abandoned_references();
    }
    drop(scope);
    failure
}

/// Duplicates owned values in an explicit Aura collection copy.
///
/// # Safety
/// The input points to a live runtime box for the duration of the call.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_copy_class_owners(
    value: *mut OpaqueValue,
) -> *mut OpaqueValue {
    let copy = aura_direct_clone_value(value);
    unsafe { value_mut(copy, crate::runtime_value::class_drop::renew_owners) };
    copy
}

/// Before recovery restores its caller, discharge the child frame's
/// abandoned references in the child. The pending task exit already holds
/// the original trap. Secondary hook failures cannot replace it.
pub(super) fn drop_abandoned_owners() {
    if !CLASS_HOOKS_ARMED.load(Ordering::Relaxed) {
        return;
    }
    release_abandoned_references();
    crate::runtime_value::class_drop::reverse_owner_order();
    let _secondary = drain_released_hooks();
}

fn release_abandoned_references() {
    let refs = with_direct_task_runtime_state(|state| state.owned_values_mut().take_all());
    without_generator_drop_close(|| {
        for (address, count) in refs {
            for _ in 0..count {
                unsafe {
                    release_untracked_value(address as *mut OpaqueValue);
                }
            }
        }
    });
}

/// Records an instance's lexical owner order in the active task.
/// # Safety
/// The compiler supplies a live runtime value.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_stamp_owner(value: *mut OpaqueValue, rank: u64) -> u64 {
    unsafe {
        with_value(value, |held| {
            crate::runtime_value::class_drop::stamp_owner(held, rank)
        })
    }
}

/// A retiring task has already handed its failure to the scheduler. Run
/// hooks queued by release of the task's ledger before retiring its state.
pub(super) fn finish_retiring_owners() {
    if CLASS_HOOKS_ARMED.load(Ordering::Relaxed) {
        crate::runtime_value::class_drop::reverse_owner_order();
        let _secondary = drain_released_hooks();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "C-unwind" fn observe(_args: *const i64, len: usize) -> *mut OpaqueValue {
        assert_eq!(len, 1);
        INVOCATIONS.set(INVOCATIONS.get() + 1);
        boxed_value(Value::Unit)
    }

    #[test]
    fn class_drop_native_abi_arms_copies_and_drains_each_owner_once() {
        with_direct_task_runtime_scope(|| unsafe {
            let receiver = boxed_value(Value::Instance(InstanceValue {
                class_name: "Ticket".into(),
                fields: Default::default(),
            }));
            INVOCATIONS.set(0);
            aura_direct_arm_class_drop(receiver, observe);
            let rank = aura_direct_stamp_owner(receiver, 0);
            assert_ne!(rank, 0);
            let copy = aura_direct_copy_class_owners(receiver);
            assert_eq!(aura_direct_stamp_owner(copy, rank), rank);
            aura_direct_release_value(receiver);
            aura_direct_drain_class_drops();
            assert_eq!(INVOCATIONS.get(), 1);
            aura_direct_release_value(copy);
            aura_direct_drain_class_drops();
            assert_eq!(INVOCATIONS.get(), 2);
            drop_abandoned_owners();
            finish_retiring_owners();
            assert_eq!(INVOCATIONS.get(), 2);
        });
    }
}
