use aurora_compiler::run_source;

const NESTED_CALL_TRAP: &str = "def explode() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef relay() -> int32:\n    return explode()\n\ndef main() -> int32:\n    return relay()\n";

const CHILD_TASK_TRAP: &str = "def child() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef main() -> int32:\n    with group = TaskGroup():\n        group.start(child)\n    return 0\n";

#[test]
fn nested_synchronous_trap_records_and_renders_the_aurora_call_chain_once() {
    let error = run_source(NESTED_CALL_TRAP).expect_err("nested call should trap");
    let chain =
        "Aurora call chain (innermost first): explode at 1:1 -> relay at 5:1 -> main at 8:1";

    assert_eq!(
        error
            .notes
            .iter()
            .filter(|note| note.starts_with("Aurora call chain"))
            .count(),
        1,
        "the runtime should annotate a propagating trap exactly once: {:?}",
        error.notes
    );
    assert!(
        error.notes.iter().any(|note| note == chain),
        "missing call-chain note: {:?}",
        error.notes
    );

    let rendered = error.render_with_source("nested.au", NESTED_CALL_TRAP);
    assert!(
        rendered.contains(&format!("note: {chain}")),
        "human diagnostic should render the call chain:\n{rendered}"
    );
}

#[test]
fn spawned_child_trap_records_and_renders_task_entry_and_spawn_ancestry() {
    let error = run_source(CHILD_TASK_TRAP).expect_err("unobserved child failure should trap");
    let call_chain = "Aurora call chain (innermost first): child at 1:1";
    let task_entry = "Aurora task entry: child at 1:1";
    let ancestry = "Aurora task ancestry (youngest first): child spawned from main at 7:15";

    for expected in [call_chain, task_entry, ancestry] {
        assert!(
            error.notes.iter().any(|note| note == expected),
            "missing `{expected}` in task diagnostic notes: {:?}",
            error.notes
        );
    }

    let rendered = error.render_with_source("task.au", CHILD_TASK_TRAP);
    for expected in [call_chain, task_entry, ancestry] {
        assert!(
            rendered.contains(&format!("note: {expected}")),
            "human diagnostic should render `{expected}`:\n{rendered}"
        );
    }
}
