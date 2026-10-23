use std::ptr::NonNull;

use super::{
    direct_frame_stack_layout, DirectFrameStack, DirectFrameStackLayout,
    DIRECT_FRAME_FLAG_PENDING_SINKS, DIRECT_FRAME_FLAG_RETURNED_VIEWS,
    DIRECT_FRAME_STACK_INLINE_ENTRIES, DIRECT_MAX_CALL_DEPTH,
};
use crate::native_runtime::{
    DirectFrameDescriptor, DirectFrameText, DirectRuntimeCallFrame, DirectRuntimeSourceSpan,
};
use aura_core::diag::Span;

/// A stack the test owns, freed when it drops.
struct OwnedStack(NonNull<DirectFrameStack>);

impl OwnedStack {
    fn new() -> Self {
        Self(DirectFrameStack::allocate())
    }

    fn raw(&self) -> *mut DirectFrameStack {
        self.0.as_ptr()
    }

    fn get(&mut self) -> &mut DirectFrameStack {
        // SAFETY: the test owns the stack and holds no other borrow.
        unsafe { &mut *self.0.as_ptr() }
    }

    fn generated_push(&self, descriptor: *const DirectFrameDescriptor) -> bool {
        // SAFETY: the stack and descriptor are live.
        unsafe { DirectFrameStack::generated_push(self.raw(), descriptor) }
    }

    fn generated_pop(&self) -> bool {
        // SAFETY: the stack is live.
        unsafe { DirectFrameStack::generated_pop(self.raw()) }
    }
}

impl Drop for OwnedStack {
    fn drop(&mut self) {
        // SAFETY: nothing uses the stack after the test drops it.
        unsafe { DirectFrameStack::free(self.0) };
    }
}

/// A validated descriptor, as the main wrapper leaves every one.
fn descriptor(function: &'static [u8]) -> *const DirectFrameDescriptor {
    let descriptor = Box::leak(Box::new(DirectFrameDescriptor::new(
        3,
        1,
        b"/workspace/main.au".as_ptr(),
        b"/workspace/main.au".len(),
        function.as_ptr(),
        function.len(),
    )));
    descriptor.ensure_validated();
    descriptor
}

fn owned_frame(name: &str) -> Box<DirectRuntimeCallFrame> {
    Box::new(DirectRuntimeCallFrame {
        function: DirectFrameText::shared(name.to_string()),
        span: DirectRuntimeSourceSpan::point(None, Span::new(1, 1)),
    })
}

fn innermost_names(stack: &mut OwnedStack) -> Vec<String> {
    stack
        .get()
        .materialize_innermost_first()
        .into_iter()
        .map(|frame| frame.function)
        .collect()
}

#[test]
fn the_layout_names_the_header_fields_generated_code_reads() {
    let layout = direct_frame_stack_layout();
    assert_eq!(
        [
            layout.len_offset,
            layout.limit_offset,
            layout.floor_offset,
            layout.flags_offset,
            layout.entries_offset,
        ],
        [0, 8, 16, 24, 32],
        "generated code relies on the header coming first"
    );
    assert_eq!(DirectFrameStackLayout::ENTRY_BYTES, 8);

    let fingerprint = layout.fingerprint();
    assert_eq!(fingerprint, direct_frame_stack_layout().fingerprint());
    let changed = [
        DirectFrameStackLayout {
            len_offset: 1,
            ..layout
        },
        DirectFrameStackLayout {
            limit_offset: 1,
            ..layout
        },
        DirectFrameStackLayout {
            floor_offset: 1,
            ..layout
        },
        DirectFrameStackLayout {
            flags_offset: 1,
            ..layout
        },
        DirectFrameStackLayout {
            entries_offset: 1,
            ..layout
        },
    ];
    for other in changed {
        assert_ne!(other.fingerprint(), fingerprint, "{other:?}");
    }
}

#[test]
fn generated_pushes_fill_the_inline_entries_then_the_runtime_grows_the_stack() {
    let mut stack = OwnedStack::new();
    let leaf = descriptor(b"leaf");
    assert_eq!(stack.get().limit(), DIRECT_FRAME_STACK_INLINE_ENTRIES);
    for _ in 0..DIRECT_FRAME_STACK_INLINE_ENTRIES {
        assert!(stack.generated_push(leaf));
    }
    assert!(
        !stack.generated_push(leaf),
        "a full block sends the push to the runtime"
    );
    assert!(!stack.get().is_spilled());

    stack
        .get()
        .push_descriptor(NonNull::new(leaf.cast_mut()).unwrap());
    assert!(stack.get().is_spilled());
    assert_eq!(
        stack.get().capacity(),
        2 * DIRECT_FRAME_STACK_INLINE_ENTRIES
    );
    assert_eq!(stack.get().len(), DIRECT_FRAME_STACK_INLINE_ENTRIES + 1);
    while stack.generated_push(leaf) {}
    assert_eq!(stack.get().len(), 2 * DIRECT_FRAME_STACK_INLINE_ENTRIES);
    stack
        .get()
        .push_descriptor(NonNull::new(leaf.cast_mut()).unwrap());
    while stack.generated_push(leaf) {}
    assert_eq!(
        stack.get().len(),
        DIRECT_MAX_CALL_DEPTH,
        "the depth limit stops generated pushes even with room left"
    );
    assert_eq!(stack.get().limit(), DIRECT_MAX_CALL_DEPTH);
    assert_eq!(stack.get().depth(), DIRECT_MAX_CALL_DEPTH);
    assert!(innermost_names(&mut stack)
        .iter()
        .all(|name| name == "leaf"));

    while stack.generated_pop() {}
    assert_eq!(stack.get().len(), 0);
    assert!(
        stack.get().is_spilled(),
        "a grown stack keeps its block, so later recursion does not allocate"
    );
    assert!(!stack.get().pop_call());
}

#[test]
fn runtime_flags_send_pushes_and_pops_to_the_runtime() {
    let mut stack = OwnedStack::new();
    let leaf = descriptor(b"leaf");
    assert!(stack.generated_push(leaf));
    for flag in [
        DIRECT_FRAME_FLAG_PENDING_SINKS,
        DIRECT_FRAME_FLAG_RETURNED_VIEWS,
    ] {
        stack.get().set_runtime_flags(flag);
        assert_eq!(stack.get().flags(), flag);
        assert!(!stack.generated_push(leaf));
        assert!(!stack.generated_pop());
        stack.get().set_runtime_flags(0);
        assert_eq!(stack.get().flags(), 0);
    }
    assert!(stack.generated_pop());
}

#[test]
fn owned_frames_keep_pops_in_the_runtime_until_they_leave() {
    let mut stack = OwnedStack::new();
    let leaf = descriptor(b"leaf");
    stack.get().push_owned(owned_frame("built"));
    assert_ne!(stack.get().flags(), 0);
    assert!(
        !stack.generated_push(leaf),
        "flags send every push to the runtime while a record is owned"
    );
    stack
        .get()
        .push_descriptor(NonNull::new(leaf.cast_mut()).unwrap());
    assert_eq!(innermost_names(&mut stack), ["leaf", "built"]);
    assert!(!stack.generated_pop());
    assert!(stack.get().pop_call());
    assert!(stack.get().pop_call());
    assert_eq!(stack.get().flags(), 0);
    assert!(stack.generated_push(leaf));
    assert!(stack.generated_pop());

    // A stack freed with records still on it releases them.
    let mut abandoned = OwnedStack::new();
    abandoned.get().push_owned(owned_frame("left behind"));
    abandoned.get().push_owned(owned_frame("also left"));
}

#[test]
fn the_floor_restarts_the_depth_without_dropping_entries() {
    let mut stack = OwnedStack::new();
    let outer = descriptor(b"outer");
    let cleanup = descriptor(b"cleanup");
    assert!(stack.generated_push(outer));
    assert!(stack.generated_push(outer));
    stack.get().set_depth(0);
    assert_eq!(stack.get().depth(), 0);
    assert!(!stack.generated_pop(), "a pop never goes below the floor");
    assert!(!stack.get().pop_call());
    assert_eq!(stack.get().limit(), DIRECT_FRAME_STACK_INLINE_ENTRIES);
    assert!(stack.generated_push(cleanup));
    assert_eq!(stack.get().depth(), 1);
    assert_eq!(innermost_names(&mut stack), ["cleanup", "outer", "outer"]);
    assert!(stack.generated_pop());
    stack.get().set_depth(2);
    assert_eq!(stack.get().depth(), 2);
    assert!(stack.generated_pop());
    assert!(stack.generated_pop());
    assert!(!stack.generated_pop());

    // Deep cleanup counts its own depth: the limit sits above the floor.
    let mut deep = OwnedStack::new();
    let leaf = descriptor(b"leaf");
    for _ in 0..DIRECT_MAX_CALL_DEPTH {
        if !deep.generated_push(leaf) {
            deep.get()
                .push_descriptor(NonNull::new(leaf.cast_mut()).unwrap());
        }
    }
    assert_eq!(deep.get().depth(), DIRECT_MAX_CALL_DEPTH);
    deep.get().set_depth(0);
    assert_eq!(deep.get().limit(), deep.get().capacity());
    deep.get()
        .push_descriptor(NonNull::new(leaf.cast_mut()).unwrap());
    assert!(deep.get().capacity() > DIRECT_MAX_CALL_DEPTH);
    assert_eq!(deep.get().limit(), deep.get().capacity());
    while deep.generated_push(leaf) {}
    assert_eq!(deep.get().depth(), DIRECT_MAX_CALL_DEPTH);
    assert_eq!(deep.get().len(), 2 * DIRECT_MAX_CALL_DEPTH);
}
