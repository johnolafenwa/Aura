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
pub unsafe extern "C-unwind" fn aura_direct_arm_class_drop(value: *mut OpaqueValue, thunk: NativeThunk) {
    task_runtime_boundary(|| unsafe {
        CLASS_HOOKS_ARMED.store(true, Ordering::Relaxed);
        value_mut(value, |value| {
            let Value::Instance(instance) = value else { runtime_error("invalid drop hook receiver") };
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
    let batch = released_instances();
    if batch.is_empty() { return None; }
    let key = direct_task_runtime_key();
    let outer = with_direct_task_runtime_state_for_key(key, |state| {
        std::mem::replace(&mut state.cleanup_draining, true)
    });
    let guard = (!outer).then(|| DirectCleanupDrainGuard { key });
    for (method, value) in batch {
        let DropMethod::Native(thunk) = method else { runtime_error("interpreted drop hook inside native code") };
        let owner = boxed_value(Value::Instance(value));
        let mut arg = owner as i64;
        // SAFETY: the program supplied this thunk for this instance.
        unsafe {
            run_drained_cleanup_thunk(key, thunk, &mut arg, 1);
            aura_direct_release_value(owner);
        }
        let _ = drain_released_hooks();
    }
    drop(guard);
    if !outer {
        with_direct_task_runtime_state_for_key(key, |state| state.primary_runtime_diagnostic.take())
    } else { None }
}

/// Duplicates owned values in an explicit Aura collection copy.
///
/// # Safety
/// The input points to a live runtime box for the duration of the call.
#[cfg_attr(not(coverage), no_mangle)]
pub unsafe extern "C-unwind" fn aura_direct_copy_class_owners(value: *mut OpaqueValue) -> *mut OpaqueValue {
    let copy = aura_direct_clone_value(value);
    unsafe { value_mut(copy, crate::runtime_value::class_drop::renew_owners) };
    copy
}

/// Before recovery restores its caller, discharge the child frame's
/// abandoned references in the child. The pending task exit already holds
/// the original trap. Secondary hook failures cannot replace it.
pub(super) fn drop_abandoned_owners() {
    if !CLASS_HOOKS_ARMED.load(Ordering::Relaxed) { return; }
    let refs = with_direct_task_runtime_state(|state| state.owned_values_mut().take_all());
    without_generator_drop_close(|| {
        for (address, count) in refs {
            for _ in 0..count {
                unsafe { release_untracked_value(address as *mut OpaqueValue); }
            }
        }
    });
    crate::runtime_value::class_drop::reverse_owner_order();
    let _secondary = drain_released_hooks();
}
