use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use super::{
    direct_ownership_layout, generated_release, generated_retain, DirectOwnedValues,
    DirectOwnershipLayout,
};
use crate::native_runtime::{share_direct_value, DirectFrameStack, OpaqueValue};
use crate::runtime_value::Value;

/// A task's frame stack with its ledger on, freed when the test drops it.
struct Task(NonNull<DirectFrameStack>);

impl Task {
    fn new() -> Self {
        let task = Self(DirectFrameStack::allocate());
        task.ledger().set_tracking(true);
        task
    }

    fn stack(&self) -> *mut DirectFrameStack {
        self.0.as_ptr()
    }

    #[allow(clippy::mut_from_ref)]
    fn ledger(&self) -> &mut DirectOwnedValues {
        // SAFETY: the test owns the stack and holds one borrow at a time.
        unsafe { &mut (*self.0.as_ptr()).owned_values }
    }

    fn add(&self, value: *mut OpaqueValue) -> bool {
        // SAFETY: the test's boxes are live and on this thread.
        unsafe { self.ledger().add(self.stack(), value) }
    }

    fn remove(&self, value: *mut OpaqueValue) -> bool {
        // SAFETY: as in `add`.
        unsafe { self.ledger().remove(self.stack(), value) }
    }

    fn retain_in_place(&self, value: *mut OpaqueValue) -> bool {
        // SAFETY: as in `add`.
        unsafe { generated_retain(self.stack(), value) }
    }

    fn release_in_place(&self, value: *mut OpaqueValue) -> bool {
        // SAFETY: as in `add`.
        unsafe { generated_release(self.stack(), value) }
    }

    fn entries(&self) -> Vec<(usize, usize)> {
        let mut entries = self.ledger().entries();
        entries.sort_unstable();
        entries
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        // SAFETY: nothing uses the stack after the test drops it.
        unsafe { DirectFrameStack::free(self.0) };
    }
}

/// A box no ledger has seen, with one reference.
fn new_box() -> *mut OpaqueValue {
    Box::into_raw(Box::new(OpaqueValue::new(Value::Unit, None)))
}

fn header(value: *mut OpaqueValue) -> &'static OpaqueValue {
    // SAFETY: the test frees each box only after its last use.
    unsafe { &*value }
}

fn free_box(value: *mut OpaqueValue) {
    // SAFETY: the test hands over its only reference.
    drop(unsafe { Box::from_raw(value) });
}

fn ref_count(value: *mut OpaqueValue) -> u32 {
    header(value).ref_count.load(Ordering::Relaxed)
}

/// Adds a reference the way a retain does: the count, then the ledger.
fn retain(task: &Task, value: *mut OpaqueValue) {
    header(value).ref_count.fetch_add(1, Ordering::Relaxed);
    assert!(task.add(value));
}

#[test]
fn the_layout_names_the_fields_generated_code_updates() {
    let layout = direct_ownership_layout();
    let mut offsets = [
        layout.ref_count_offset,
        layout.shared_lock_offset,
        layout.owner_offset,
        layout.owned_offset,
    ];
    offsets.sort_unstable();
    offsets
        .windows(2)
        .for_each(|pair| assert!(pair[0] < pair[1]));
    let fingerprint = layout.fingerprint();
    assert_eq!(fingerprint, direct_ownership_layout().fingerprint());
    for other in [
        DirectOwnershipLayout {
            ref_count_offset: -1,
            ..layout
        },
        DirectOwnershipLayout {
            shared_lock_offset: -1,
            ..layout
        },
        DirectOwnershipLayout {
            owner_offset: -1,
            ..layout
        },
        DirectOwnershipLayout {
            owned_offset: -1,
            ..layout
        },
    ] {
        assert_ne!(other.fingerprint(), fingerprint, "{other:?}");
    }
}

#[test]
fn a_confined_value_links_into_its_tasks_ledger_and_generated_code_counts_it() {
    let task = Task::new();
    let value = new_box();
    assert!(
        !task.retain_in_place(value),
        "generated code leaves a value no ledger links to the runtime"
    );
    assert!(task.add(value));
    assert_eq!(header(value).owner.get(), task.stack());
    assert_eq!(task.entries(), [(value as usize, 1)]);
    assert!(
        !task.release_in_place(value),
        "the last reference goes through the runtime, which unlinks it"
    );

    assert!(task.retain_in_place(value));
    assert!(task.retain_in_place(value));
    assert_eq!((ref_count(value), header(value).owned.get()), (3, 3));
    assert_eq!(task.entries(), [(value as usize, 3)]);
    assert!(task.release_in_place(value));
    assert!(task.release_in_place(value));
    assert_eq!((ref_count(value), header(value).owned.get()), (1, 1));
    assert!(!task.release_in_place(value));

    assert!(task.remove(value));
    assert!(task.ledger().is_empty());
    assert!(header(value).owner.get().is_null());
    assert!(!task.remove(value), "the ledger owns no reference any more");
    free_box(value);
}

#[test]
fn generated_code_leaves_a_full_or_shared_count_to_the_runtime() {
    let task = Task::new();
    let value = new_box();
    retain(&task, value);
    header(value).ref_count.store(u32::MAX, Ordering::Relaxed);
    assert!(
        !task.retain_in_place(value),
        "the runtime reports the overflow"
    );
    header(value).ref_count.store(2, Ordering::Relaxed);
    // An owned count of one with other references still leaves the
    // runtime to unlink the value.
    assert!(task.remove(value));
    retain(&task, value);
    assert_eq!(header(value).owned.get(), 1);
    assert!(!task.release_in_place(value));
    assert!(task.remove(value));
    header(value).ref_count.store(1, Ordering::Relaxed);
    free_box(value);
}

#[test]
fn values_unlink_in_any_order() {
    let task = Task::new();
    let values = [new_box(), new_box(), new_box(), new_box()];
    for value in values {
        retain(&task, value);
    }
    let mut expected = values.map(|value| (value as usize, 1)).to_vec();
    expected.sort_unstable();
    assert_eq!(task.entries(), expected);
    // The middle, the head, the tail, then the last one.
    for index in [1, 3, 0, 2] {
        assert!(task.remove(values[index]));
        expected.retain(|(address, _)| *address != values[index] as usize);
        assert_eq!(task.entries(), expected);
    }
    assert!(task.ledger().is_empty());
    for value in values {
        free_box(value);
    }
}

#[test]
fn shared_values_count_by_address() {
    let task = Task::new();
    let shared = new_box();
    // SAFETY: the box is live and on this thread.
    unsafe { share_direct_value(shared) };
    retain(&task, shared);
    assert!(
        header(shared).owner.get().is_null(),
        "a shared value never links"
    );
    assert_eq!(task.entries(), [(shared as usize, 1)]);
    assert!(!task.retain_in_place(shared));
    assert!(!task.release_in_place(shared));
    assert!(task.remove(shared));
    assert!(task.ledger().is_empty());
    assert!(!task.remove(shared));

    // Sharing a linked value moves its count, since other threads may read
    // the header from now on.
    let linked = new_box();
    retain(&task, linked);
    retain(&task, linked);
    let other = new_box();
    retain(&task, other);
    // SAFETY: as above.
    unsafe { share_direct_value(linked) };
    assert!(header(linked).owner.get().is_null());
    let mut expected = vec![(linked as usize, 2), (other as usize, 1)];
    expected.sort_unstable();
    assert_eq!(task.entries(), expected);
    assert!(!task.retain_in_place(linked));
    assert!(task.remove(linked));
    assert!(task.remove(linked));
    assert!(task.remove(other));
    assert!(task.ledger().is_empty());
    for value in [shared, linked, other] {
        header(value).ref_count.store(1, Ordering::Relaxed);
        free_box(value);
    }
}

#[test]
fn a_value_another_task_links_counts_by_address() {
    let first = Task::new();
    let second = Task::new();
    let value = new_box();
    retain(&first, value);
    retain(&second, value);
    retain(&second, value);
    assert_eq!(header(value).owner.get(), first.stack());
    assert_eq!(second.entries(), [(value as usize, 2)]);
    assert!(
        !second.retain_in_place(value),
        "generated code counts only the running task's linked values"
    );
    assert!(second.remove(value));
    assert!(second.remove(value));
    assert!(!second.remove(value));
    assert!(first.remove(value));
    header(value).ref_count.store(1, Ordering::Relaxed);
    free_box(value);
}

#[test]
fn a_finished_task_hands_back_or_forgets_every_reference() {
    let task = Task::new();
    let linked = new_box();
    let shared = new_box();
    // SAFETY: the box is live and on this thread.
    unsafe { share_direct_value(shared) };
    retain(&task, linked);
    retain(&task, linked);
    retain(&task, shared);
    let mut taken = task.ledger().take_all();
    taken.sort_unstable();
    let mut expected = vec![(linked as usize, 2), (shared as usize, 1)];
    expected.sort_unstable();
    assert_eq!(taken, expected);
    assert!(task.ledger().is_empty());
    assert!(header(linked).owner.get().is_null());

    // A task state dropped without its ledger drained forgets the links,
    // so no value names the freed stack.
    let dropped = Task::new();
    let kept = new_box();
    retain(&dropped, kept);
    drop(dropped);
    assert!(header(kept).owner.get().is_null());
    for value in [linked, shared, kept] {
        header(value).ref_count.store(1, Ordering::Relaxed);
        free_box(value);
    }
}

#[test]
fn a_value_freed_while_linked_leaves_the_list() {
    let task = Task::new();
    let freed = new_box();
    let kept = new_box();
    retain(&task, kept);
    retain(&task, freed);
    free_box(freed);
    assert_eq!(task.entries(), [(kept as usize, 1)]);
    assert!(task.remove(kept));
    free_box(kept);
}

#[test]
fn counts_that_would_overflow_are_refused() {
    let task = Task::new();
    let linked = new_box();
    assert!(task.add(linked));
    header(linked).owned.set(usize::MAX);
    assert!(!task.add(linked));
    header(linked).owned.set(1);
    assert!(task.remove(linked));

    let shared = new_box();
    // SAFETY: the box is live and on this thread.
    unsafe { share_direct_value(shared) };
    assert!(task.add(shared));
    let mut others = std::collections::HashMap::default();
    others.insert(shared as usize, usize::MAX);
    task.ledger().install_others(others);
    assert!(!task.add(shared));
    task.ledger().install_others(Default::default());
    assert_eq!(task.ledger().count(shared), 0);
    free_box(linked);
    free_box(shared);
}
