use super::*;

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn call(name: &str, args: Vec<Value>) -> Result<Value> {
    sync_host_function(name).expect("a sync host function")(args, None)
}

fn as_i64(value: &Value) -> i64 {
    match value {
        Value::Int(value) => i64::try_from(value.as_i128().expect("an integer")).expect("an int64"),
        other => panic!("expected an integer, found {other:?}"),
    }
}

#[test]
fn a_cell_gives_its_value_out_and_takes_it_back() {
    let cell = call("sync::__aura_cell_new", vec![int(5)]).expect("a cell");
    let Value::Sync(handle) = &cell else {
        panic!("expected a sync handle");
    };
    assert_eq!(handle.type_name(), "sync.Cell");
    assert_eq!(format!("{handle:?}"), "sync.Cell");
    let held = call("sync::__aura_cell_take", vec![cell.clone()]).expect("the value");
    assert_eq!(as_i64(&held), 5);
    assert!(matches!(
        call("sync::__aura_cell_poisoned", vec![cell.clone()]),
        Ok(Value::Bool(false))
    ));
    call("sync::__aura_cell_put", vec![cell.clone(), int(6)]).expect("a release");
    let held = handle.take().expect("the value again");
    assert_eq!(as_i64(&held), 6);
    handle.put(held).expect("a second release");
    assert!(!handle.is_poisoned().expect("a cell"));
}

#[test]
fn a_holder_cannot_take_its_own_cell_twice() {
    let cell = SyncValue::cell(int(1));
    let held = cell.take().expect("the value");
    let error = cell.take().expect_err("a second take by the holder");
    assert_eq!(error.code, "AU4001");
    assert!(error.message.contains("already holds the lock"));
    cell.put(held).expect("a release");
}

#[test]
fn a_release_by_a_task_that_does_not_hold_the_cell_is_refused() {
    let cell = SyncValue::cell(int(1));
    let error = cell.put(int(2)).expect_err("a release without a take");
    assert!(error.message.contains("does not hold it"));
    let held = cell.take().expect("the value");
    let other = cell.clone();
    let refused = std::thread::spawn(move || other.put(int(3)))
        .join()
        .expect("the other thread");
    assert!(refused.is_err());
    cell.put(held).expect("the holder's release");
}

#[test]
fn threads_take_turns_on_one_cell() {
    let cell = SyncValue::cell(int(0));
    let workers = (0..4)
        .map(|_| {
            let cell = cell.clone();
            std::thread::spawn(move || {
                for _ in 0..2_000 {
                    let held = cell.take().expect("the value");
                    let next = as_i64(&held) + 1;
                    cell.put(int(next)).expect("a release");
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().expect("a worker");
    }
    let held = cell.take().expect("the value");
    assert_eq!(as_i64(&held), 8_000);
    cell.put(held).expect("a release");
}

#[test]
fn a_holder_that_ends_poisons_the_cell_and_wakes_its_waiters() {
    let cell = SyncValue::cell(int(1));
    let (taken, wait_for_taken) = std::sync::mpsc::channel();
    let (finish, wait_to_finish) = std::sync::mpsc::channel::<()>();
    let holder_cell = cell.clone();
    let holder = std::thread::spawn(move || {
        let _lost = holder_cell.take().expect("the value");
        taken.send(()).expect("the main thread");
        wait_to_finish.recv().expect("the main thread");
        // What the scheduler does when a task ends inside a lock.
        THREAD_HOLDER.with(holder_ended);
    });
    wait_for_taken.recv().expect("the holder");
    assert!(!cell.is_poisoned().expect("a cell"));
    let waiter_cell = cell.clone();
    let waiter = std::thread::spawn(move || waiter_cell.take());
    // Let the waiter reach its wait, then end the holder.
    std::thread::sleep(StdDuration::from_millis(50));
    finish.send(()).expect("the holder");
    holder.join().expect("the holder");
    let error = waiter
        .join()
        .expect("the waiter")
        .expect_err("a take of a poisoned cell");
    assert_eq!(error.code, "AU4001");
    assert!(error.message.contains("poisoned"));
    assert!(cell.is_poisoned().expect("a cell"));
    assert!(cell.take().is_err());
}

#[test]
fn only_a_task_that_acquired_a_cell_installs_exit_cleanup() {
    let active = HolderState::default();
    let idle = HolderState::default();
    assert!(active.on_exit.get().is_none());
    let guarded = SyncValue::cell(int(7));
    let cell = guarded.as_cell("lock").unwrap();
    assert!(cell.try_lock());
    active.acquired(cell);
    assert!(active.on_exit.get().is_some());
    holder_ended(&idle);
    assert!(idle.on_exit.get().is_none());
    assert!(!guarded.is_poisoned().unwrap());
    holder_ended(&active);
    assert!(guarded.is_poisoned().unwrap());
}

#[test]
fn a_holder_that_ends_with_nothing_held_poisons_nothing() {
    let holder = HolderState::default();
    holder_ended(&holder);
    let cell = SyncValue::cell(int(1));
    let held = cell.take().expect("the value");
    cell.put(held).expect("a release");
    assert!(!cell.is_poisoned().expect("a cell"));
}

#[test]
fn an_atom_loads_stores_adds_swaps_and_compares() {
    let atom = call("sync::__aura_atom_new", vec![int(5)]).expect("an atom");
    let Value::Sync(handle) = &atom else {
        panic!("expected a sync handle");
    };
    assert_eq!(handle.type_name(), "sync.Atom");
    let load = |atom: &Value| as_i64(&call("sync::__aura_atom_load", vec![atom.clone()]).unwrap());
    assert_eq!(load(&atom), 5);
    call("sync::__aura_atom_store", vec![atom.clone(), int(7)]).expect("a store");
    assert_eq!(
        as_i64(&call("sync::__aura_atom_add", vec![atom.clone(), int(3)]).unwrap()),
        10
    );
    assert_eq!(
        as_i64(&call("sync::__aura_atom_swap", vec![atom.clone(), int(1)]).unwrap()),
        10
    );
    assert!(matches!(
        call(
            "sync::__aura_atom_compare_and_set",
            vec![atom.clone(), int(1), int(2)]
        ),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        call(
            "sync::__aura_atom_compare_and_set",
            vec![atom.clone(), int(1), int(3)]
        ),
        Ok(Value::Bool(false))
    ));
    assert_eq!(load(&atom), 2);
}

#[test]
fn an_atom_add_that_overflows_traps() {
    let atom = SyncValue::atom(i64::MAX);
    let error = atom.add(1).expect_err("an overflow");
    assert_eq!(error.code, "AU4002");
    assert_eq!(atom.load().expect("an atom"), i64::MIN);
}

#[test]
fn a_handle_of_the_wrong_kind_is_refused() {
    let cell = SyncValue::cell(int(1));
    let atom = SyncValue::atom(1);
    assert!(cell.load().is_err());
    assert!(cell.store(1).is_err());
    assert!(cell.add(1).is_err());
    assert!(cell.swap(1).is_err());
    assert!(cell.compare_and_set(1, 2).is_err());
    assert!(atom.take().is_err());
    assert!(atom.put(int(1)).is_err());
    assert!(atom.is_poisoned().is_err());
    assert!(cell != atom);
    assert!(cell == cell.clone());
    assert!(atom == atom.clone());
    assert!(cell != SyncValue::cell(int(1)));
    assert!(atom != SyncValue::atom(1));
    assert!(call("sync::__aura_atom_load", vec![int(1)]).is_err());
    assert!(call(
        "sync::__aura_atom_store",
        vec![Value::Sync(atom), Value::Bool(true)]
    )
    .is_err());
    assert!(sync_host_function("sync::__aura_missing").is_none());
}

#[test]
fn every_sync_host_function_checks_its_argument_count() {
    for name in [
        "sync::__aura_cell_new",
        "sync::__aura_cell_take",
        "sync::__aura_cell_put",
        "sync::__aura_cell_poisoned",
        "sync::__aura_map_get",
        "sync::__aura_atom_new",
        "sync::__aura_atom_load",
        "sync::__aura_atom_store",
        "sync::__aura_atom_add",
        "sync::__aura_atom_swap",
        "sync::__aura_atom_compare_and_set",
    ] {
        assert!(call(name, Vec::new()).is_err(), "{name}");
    }
}

#[test]
fn a_shared_map_lookup_copies_its_entry_and_releases_the_lock() {
    let table = Value::Map(MapValue {
        key_type: SharedType::named("str"),
        value_type: SharedType::named("int64"),
        entries: vec![(Value::string("key"), int(7))].into(),
    });
    let cell = Value::Sync(SyncValue::cell(table));
    for (key, expected) in [("key", lookup_found(int(7))), ("missing", lookup_missing())] {
        let result = call(
            "sync::__aura_map_get",
            vec![cell.clone(), Value::string(key)],
        )
        .unwrap();
        assert_eq!(result, expected);
    }
    let invalid = SyncValue::cell(Value::Unit);
    let error = call(
        "sync::__aura_map_get",
        vec![Value::Sync(invalid.clone()), int(1)],
    )
    .unwrap_err();
    assert!(error.message.contains("dictionary"));
    abandon_held_cells();
    assert!(invalid.is_poisoned().unwrap());
}

#[test]
fn refusing_a_release_preserves_other_locks_held_by_the_task() {
    let held = SyncValue::cell(int(7));
    let unrelated = SyncValue::cell(int(9));
    held.take().unwrap();
    assert!(unrelated.put(int(8)).is_err());
    abandon_held_cells();
    assert!(held.is_poisoned().unwrap());
}

#[test]
fn failure_inside_a_lock_scope_only_poisons_its_own_locks() {
    let outer = SyncValue::cell(int(11));
    let inner = SyncValue::cell(int(17));
    let held = outer.take().unwrap();
    let boundary = mark_held_cells();
    inner.take().unwrap();
    abandon_held_cells_since(boundary);
    assert!(inner.is_poisoned().unwrap());
    assert!(!outer.is_poisoned().unwrap());
    outer.put(held).unwrap();
    assert_eq!(with_current_holder(|holder| holder.depth.get()), 0);
}

#[test]
fn a_released_outer_lock_does_not_hide_a_new_lock_from_recovery() {
    let outer = SyncValue::cell(Value::Unit);
    let inner = SyncValue::cell(Value::Unit);
    let outer_value = outer.take().unwrap();
    let boundary = mark_held_cells();
    outer.put(outer_value).unwrap();
    inner.take().unwrap();
    abandon_held_cells_since(boundary);
    assert!(inner.is_poisoned().unwrap());
    assert!(!outer.is_poisoned().unwrap());
}

#[test]
fn returning_the_outer_of_two_held_cells_keeps_the_inner_registered() {
    let outer = SyncValue::cell(Value::Unit);
    let inner = SyncValue::cell(Value::Unit);
    let held = outer.take().unwrap();
    let boundary = mark_held_cells();
    inner.take().unwrap();
    outer.put(held).unwrap();
    assert!(inner.take().unwrap_err().message.contains("already holds"));
    outer.take().unwrap();
    abandon_held_cells_since(boundary);
    assert!(inner.is_poisoned().unwrap());
    assert!(outer.is_poisoned().unwrap());
}

#[test]
fn standard_mutex_and_atomic_wrappers_render_as_opaque_handles() {
    for name in [
        "sync.Mutex",
        "sync.Locked",
        "sync.Map",
        "sync.AtomicInt",
        "sync.AtomicBool",
    ] {
        assert_eq!(
            Value::Instance(InstanceValue {
                class_name: RuntimeName::from(name),
                fields: InstanceFields::default(),
            })
            .render(),
            "<sync-handle>"
        );
    }
    assert_eq!(
        Value::Instance(InstanceValue {
            class_name: RuntimeName::from_static("Mutex"),
            fields: InstanceFields::default(),
        })
        .render(),
        "Mutex()"
    );
}
