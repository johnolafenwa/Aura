use super::*;
use crate::runtime_value::run_lightweight_root_task;

fn instance(class: &str, fields: &[(&str, Value)]) -> Value {
    Value::Instance(InstanceValue {
        class_name: class.to_string().into(),
        fields: fields
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect(),
    })
}

fn sequence(class: &str, descending: bool) -> Value {
    let store = Value::Vec(VecValue::new(
        Type::named("collections.__Store"),
        Vec::<Value>::new().into(),
    ));
    instance(
        class,
        &[("store", store), ("descending", Value::Bool(descending))],
    )
}

fn lookup(receiver: *mut OpaqueValue, op: I64Op, key: i64) -> String {
    let result = aura_direct_ordered_i64_lookup(receiver, op as i64, key, 0);
    // SAFETY: the call transferred a live result and the read ends before release.
    unsafe {
        let text = with_value(result, Value::render);
        aura_direct_release_value(result);
        text
    }
}

#[test]
fn ordered_receiver_keeps_lookups_snapshots_and_nested_set_updates_consistent() {
    let initial = sequence("collections.SortedMap", false);
    let map = boxed_value(initial.clone());
    let op = |receiver, operation: I64Op, key, value| {
        aura_direct_ordered_i64(receiver, operation as i64, key, value)
    };
    assert_eq!(lookup(map, I64Op::MapGet, 7), "Lookup.Missing");
    assert_eq!(lookup(map, I64Op::MapRemove, 7), "Lookup.Missing");
    op(map, I64Op::MapSet, 7, 11);
    // SAFETY: the original owns a live receiver, whose fields copy on write.
    let copy = unsafe { with_value(map, |value| boxed_value(value.clone())) };
    op(copy, I64Op::MapSet, 7, 99);
    assert_eq!(op(map, I64Op::MapContains, 7, 0), 1);
    assert_eq!(lookup(copy, I64Op::MapGet, 7), "Lookup.Found(99)");
    assert_eq!(lookup(map, I64Op::MapRemove, 7), "Lookup.Found(11)");
    assert_eq!(op(map, I64Op::MapContains, 7, 0), 0);

    let set = boxed_value(instance("collections.SortedSet", &[("map", initial)]));
    assert_eq!(op(set, I64Op::SetRemove, 4, 0), 0);
    op(set, I64Op::SetAdd, 4, 0);
    op(set, I64Op::SetAdd, 4, 0);
    assert_eq!(op(set, I64Op::SetContains, 4, 0), 1);
    assert_eq!(op(set, I64Op::SetRemove, 4, 0), 1);
    assert_eq!(op(set, I64Op::SetRemove, 4, 0), 0);
    // SAFETY: each allocation is owned exactly once here.
    unsafe {
        for ptr in [map, copy, set] {
            aura_direct_release_value(ptr)
        }
    }
}

#[test]
fn ordered_sequence_receiver_declines_shared_backing_and_keeps_guarded_edits() {
    for (class, push, pop, descending, expected) in [
        (
            "collections.Deque",
            I64Op::PushBack,
            I64Op::PopFront,
            false,
            8,
        ),
        (
            "collections.Heap",
            I64Op::HeapPush,
            I64Op::HeapPop,
            false,
            3,
        ),
        ("collections.Heap", I64Op::HeapPush, I64Op::HeapPop, true, 8),
    ] {
        let receiver = boxed_value(sequence(class, descending));
        assert!(aura_direct_ordered_local_words(receiver, pop as i64).is_null());
        let words = aura_direct_ordered_local_words(receiver, push as i64);
        assert!(!words.is_null());
        // SAFETY: the fresh receiver is exclusive and live until the last step.
        unsafe {
            aura_direct_ordered_local_step(words, push as i64, 8);
            aura_direct_ordered_local_step(words, push as i64, 3);
            assert_eq!(
                aura_direct_ordered_local_step(words, pop as i64, 0),
                expected
            );
            // The packed address is never used after publication to the shared ABI.
            share_direct_value(receiver);
        }
        assert!(aura_direct_ordered_local_words(receiver, push as i64).is_null());
        assert_eq!(
            aura_direct_ordered_i64(receiver, pop as i64, 0, 0),
            11 - expected
        );
        // SAFETY: receiver is the last owner after the guarded operation.
        unsafe { aura_direct_release_value(receiver) };
    }
}

#[test]
fn ordered_receiver_reports_malformed_storage_and_definition_frames() {
    let unknown = instance("collections.SortedSet", &[]);
    let wrong_field = instance("collections.SortedMap", &[("store", Value::Unit)]);
    let items = Value::Vec(VecValue::new(
        Type::named("collections.__Store"),
        vec![Value::Unit].into(),
    ));
    let wrong_entry = instance("collections.SortedMap", &[("store", items)]);
    for (mut value, nested) in [
        (Value::Unit, false),
        (unknown, true),
        (wrong_field, false),
        (wrong_entry, false),
    ] {
        assert_eq!(
            read(&value, I64Op::MapGet, 0, nested).unwrap_err().code,
            "AU4001"
        );
        let failure = edit_store::<i64>(&mut value, I64Op::MapSet, nested, |_| {
            panic!("an invalid receiver cannot expose a collection store")
        })
        .unwrap_err();
        assert_eq!(failure.code, "AU4001");
    }

    for (class, remove, before, name) in [
        (
            "collections.Deque",
            I64Op::PopFront,
            None,
            "collections::Deque.pop_front",
        ),
        (
            "collections.Deque",
            I64Op::PopBack,
            Some(I64Op::PushBack),
            "collections::Deque.pop_back",
        ),
        (
            "collections.Heap",
            I64Op::HeapPop,
            None,
            "collections::Heap.pop",
        ),
        (
            "collections.Heap",
            I64Op::HeapPop,
            Some(I64Op::HeapPush),
            "collections::Heap.pop",
        ),
    ] {
        let receiver = boxed_value(sequence(class, false));
        let receiver_address = receiver as usize;
        let diagnostic = run_lightweight_root_task(move || {
            with_direct_task_runtime_scope(|| {
                with_task_runtime_error_capture(|| {
                    let receiver = receiver_address as *mut OpaqueValue;
                    let site = I64Op::pop_site(Span::new(27, 6));
                    if let Some(push) = before {
                        let words = aura_direct_ordered_local_words(receiver, push as i64);
                        // SAFETY: only these calls use this fresh receiver's packed storage.
                        unsafe {
                            aura_direct_ordered_local_step(words, push as i64, 22);
                            aura_direct_ordered_local_step(words, remove as i64, site);
                            aura_direct_ordered_local_step(words, remove as i64, site);
                        }
                    } else {
                        aura_direct_ordered_i64(receiver, remove as i64, site, 0);
                    }
                    Ok(Value::Unit)
                })
            })
        })
        .expect_err("an empty pop must reach the task boundary");
        // SAFETY: the completed task borrowed the receiver and never retained it.
        unsafe { aura_direct_release_value(receiver) };
        assert_eq!(diagnostic.code, "AU4003");
        assert_eq!(diagnostic.call_frames[0].function, name);
        assert_eq!(diagnostic.call_frames[0].span.start, Span::new(27, 6));
    }
}

#[test]
fn rejected_native_instructions_preserve_the_live_deque() {
    for entry in ["guarded", "open", "step"] {
        let receiver = boxed_value(sequence("collections.Deque", false));
        aura_direct_ordered_i64(receiver, I64Op::PushBack as i64, 41, 0);
        let address = receiver as usize;
        let error = run_lightweight_root_task(move || {
            with_direct_task_runtime_scope(|| {
                with_task_runtime_error_capture(|| {
                    let receiver = address as *mut OpaqueValue;
                    match entry {
                        "guarded" => {
                            aura_direct_ordered_i64(receiver, -1, 0, 0);
                        }
                        "open" => {
                            // Map lookups cannot open a deque's packed sequence.
                            aura_direct_ordered_local_words(receiver, I64Op::MapGet as i64);
                        }
                        _ => {
                            let words =
                                aura_direct_ordered_local_words(receiver, I64Op::PushBack as i64);
                            assert!(!words.is_null());
                            // SAFETY: the acquired address is exclusive and the receiver stays live.
                            unsafe { aura_direct_ordered_local_step(words, -1, 0) };
                        }
                    }
                    Ok(Value::Unit)
                })
            })
        })
        .expect_err("invalid instructions are reported to the task");
        assert_eq!(error.code, "AU4001");
        assert_eq!(error.message, "invalid native ordered collection receiver");
        assert_eq!(
            aura_direct_ordered_i64(receiver, I64Op::PopFront as i64, 0, 0),
            41
        );
        // SAFETY: the completed task borrowed the receiver for the call.
        unsafe { aura_direct_release_value(receiver) };
    }
}

#[test]
fn packed_deque_open_declines_a_typed_receiver_without_storage() {
    use crate::native_runtime::class_layouts::{
        register_class_layout, TypedFields, FIRST_CLASS_ACCESS,
    };
    use aura_core::layout::{encode_descriptor, ClassDefinition, LayoutEnv};

    let name = "OrderedReceiverWithoutStore";
    let mut env = LayoutEnv::default();
    env.classes.insert(
        name.into(),
        ClassDefinition {
            type_params: vec![],
            fields: vec![("tag".into(), Type::named("int64"))],
        },
    );
    let descriptor = encode_descriptor(&Type::named(name), &env).unwrap();
    let layout = register_class_layout(&descriptor, FIRST_CLASS_ACCESS + 0x0D00_0000).unwrap();
    let receiver = boxed_value(Value::Instance(InstanceValue {
        class_name: name.into(),
        fields: InstanceFields::Typed(TypedFields::new(layout)),
    }));
    assert!(aura_direct_ordered_local_words(receiver, I64Op::PushFront as i64).is_null());
    // SAFETY: the declined borrow retained no reference to the receiver.
    unsafe { aura_direct_release_value(receiver) };
}

#[test]
fn native_empty_pop_keeps_its_registered_source_path() {
    const CHILD: &str = "AURA_TEST_ORDERED_POP_SOURCE";
    if std::env::var_os(CHILD).is_none() {
        // Program source is registered once per process, as in a compiled program.
        let thread = std::thread::current();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", thread.name().unwrap(), "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let path = "deque.au";
    let source = b"def main():\n    pass\n";
    aura_direct_runtime_init(path.as_ptr(), path.len(), source.as_ptr(), source.len());
    let receiver = boxed_value(sequence("collections.Deque", false));
    let address = receiver as usize;
    let error = run_lightweight_root_task(move || {
        with_direct_task_runtime_scope(|| {
            with_task_runtime_error_capture(|| {
                let site = I64Op::pop_site(Span::new(2, 5));
                aura_direct_ordered_i64(
                    address as *mut OpaqueValue,
                    I64Op::PopBack as i64,
                    site,
                    0,
                );
                Ok(Value::Unit)
            })
        })
    })
    .expect_err("an empty deque must trap");
    // SAFETY: the task has finished borrowing this receiver.
    unsafe { aura_direct_release_value(receiver) };
    assert_eq!(error.code, "AU4003");
    assert_eq!(error.call_frames[0].function, "collections::Deque.pop_back");
    assert_eq!(error.call_frames[0].span.path.as_deref(), Some(path));
    assert_eq!(error.call_frames[0].span.start, Span::new(2, 5));
}
