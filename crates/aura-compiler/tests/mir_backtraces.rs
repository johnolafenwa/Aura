use aura_compiler::{run_path, run_source};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const NESTED_CALL_TRAP: &str = "def explode() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef relay() -> int32:\n    return explode()\n\ndef main() -> int32:\n    return relay()\n";

const CHILD_TASK_TRAP: &str = "def child() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef main() -> int32:\n    with group = TaskGroup():\n        group.start(child)\n    return 0\n";
const NESTED_TASK_TRAP: &str = "def leaf() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef child() -> int32:\n    with inner = TaskGroup():\n        inner.start(leaf)\n    return 0\n\ndef main() -> int32:\n    with outer = TaskGroup():\n        outer.start(child)\n    return 0\n";
const RECURSION_TRAP: &str = "def recurse(value: int32) -> int32:\n    return recurse(value + 1)\n\ndef main() -> int32:\n    return recurse(0)\n";
const BODY_TRAP_DURING_TASK_GROUP_CLEANUP: &str = "def child() -> int32:\n    sleep(5s)\n    return 0\n\ndef main() -> int32:\n    values: Vec[int32] = [1]\n    with group = TaskGroup():\n        group.start(child)\n        return values[9]\n";
const CLEANUP_PRIMARY_TRAP: &str = "class Resource:\n    def close(mut self):\n        values: Vec[int32] = [1]\n        print(values[9])\n\ndef main() -> int32:\n    with resource = Resource():\n        pass\n    return 0\n";

#[test]
fn nested_synchronous_trap_records_and_renders_the_aura_call_chain_once() {
    let error = run_source(NESTED_CALL_TRAP).expect_err("nested call should trap");
    let chain = "Aura call chain (innermost first): explode at 1:1 -> relay at 5:1 -> main at 8:1";

    assert_eq!(
        error
            .notes
            .iter()
            .filter(|note| note.starts_with("Aura"))
            .count(),
        0
    );
    assert_eq!(
        error
            .call_frames
            .iter()
            .map(|frame| (frame.function.as_str(), frame.span.start))
            .collect::<Vec<_>>(),
        vec![
            ("explode", aura_compiler::Span::new(1, 1)),
            ("relay", aura_compiler::Span::new(5, 1)),
            ("main", aura_compiler::Span::new(8, 1)),
        ]
    );
    assert!(error
        .call_frames
        .iter()
        .all(|frame| frame.span.path.is_none()));
    assert!(error.task_ancestry.is_empty());
    assert!(error
        .structured("nested.au")
        .call_frames
        .iter()
        .all(|frame| frame.span.path == "nested.au"));

    let rendered = error.render_with_source("nested.au", NESTED_CALL_TRAP);
    assert!(
        rendered.contains(&format!("note: {chain}")),
        "human diagnostic should render the call chain:\n{rendered}"
    );
}

#[test]
fn spawned_child_trap_records_and_renders_task_entry_and_spawn_ancestry() {
    let error = run_source(CHILD_TASK_TRAP).expect_err("unobserved child failure should trap");
    let call_chain = "Aura call chain (innermost first): child at 1:1";
    let task_entry = "Aura task entry: child at 1:1";
    let ancestry = "Aura task ancestry (youngest first): child spawned from main at 7:15";

    assert!(error.notes.iter().all(|note| !note.starts_with("Aura")));
    assert_eq!(error.call_frames.len(), 1);
    assert_eq!(error.call_frames[0].function, "child");
    assert_eq!(error.task_ancestry.len(), 1);
    assert_eq!(error.task_ancestry[0].task_function, "child");
    assert_eq!(error.task_ancestry[0].parent_function, "main");
    assert_eq!(
        error.task_ancestry[0].spawn_span.start,
        aura_compiler::Span::new(7, 15)
    );

    let rendered = error.render_with_source("task.au", CHILD_TASK_TRAP);
    for expected in [call_chain, task_entry, ancestry] {
        assert!(
            rendered.contains(&format!("note: {expected}")),
            "human diagnostic should render `{expected}`:\n{rendered}"
        );
    }
}

#[test]
fn nested_task_failure_keeps_youngest_first_typed_ancestry_exactly_once() {
    let error = run_source(NESTED_TASK_TRAP).expect_err("nested child failure should trap");
    assert_eq!(
        error
            .call_frames
            .iter()
            .map(|frame| frame.function.as_str())
            .collect::<Vec<_>>(),
        vec!["leaf"]
    );
    assert_eq!(
        error
            .task_ancestry
            .iter()
            .map(|frame| {
                (
                    frame.task_function.as_str(),
                    frame.parent_function.as_str(),
                    frame.spawn_span.start,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("leaf", "child", aura_compiler::Span::new(7, 15)),
            ("child", "main", aura_compiler::Span::new(12, 15)),
        ]
    );
    assert!(error.notes.iter().all(|note| !note.starts_with("Aura")));

    let rendered = error.render_with_source("nested-task.au", NESTED_TASK_TRAP);
    assert_eq!(rendered.matches("Aura call chain").count(), 1);
    assert_eq!(rendered.matches("Aura task entry").count(), 1);
    assert_eq!(rendered.matches("Aura task ancestry").count(), 1);
    assert!(rendered.contains(
        "Aura task ancestry (youngest first): leaf spawned from child at 7:15 -> child spawned from main at 12:15"
    ));
}

#[test]
fn imported_call_and_cross_module_task_frames_keep_their_own_paths() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aura-mir-frame-paths-{}-{unique}",
        std::process::id()
    ));
    let helpers = root.join("helpers");
    fs::create_dir_all(&helpers).expect("temporary module directory should be created");
    let worker_path = helpers.join("worker.au");
    fs::write(
        &worker_path,
        "public def explode() -> int32:\n    values: Vec[int32] = [1]\n    return values[9]\n",
    )
    .expect("worker module should be written");

    let sync_path = root.join("sync.au");
    fs::write(
        &sync_path,
        "import helpers.worker\n\ndef relay() -> int32:\n    return helpers.worker.explode()\n\ndef main() -> int32:\n    return relay()\n",
    )
    .expect("sync entry should be written");
    let canonical_worker_path =
        fs::canonicalize(&worker_path).expect("worker path should canonicalize");
    let canonical_sync_path = fs::canonicalize(&sync_path).expect("sync path should canonicalize");
    let sync_error = run_path(&sync_path).expect_err("imported nested call should trap");
    assert_eq!(
        sync_error
            .call_frames
            .iter()
            .map(|frame| (frame.function.clone(), frame.span.path.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                "helpers.worker::explode".to_string(),
                Some(canonical_worker_path.to_string_lossy().into_owned())
            ),
            (
                "relay".to_string(),
                Some(canonical_sync_path.to_string_lossy().into_owned())
            ),
            (
                "main".to_string(),
                Some(canonical_sync_path.to_string_lossy().into_owned())
            ),
        ]
    );

    let task_path = root.join("task.au");
    fs::write(
        &task_path,
        "import helpers.worker\n\ndef main() -> int32:\n    with group = TaskGroup():\n        group.start(helpers.worker.explode)\n    return 0\n",
    )
    .expect("task entry should be written");
    let canonical_task_path = fs::canonicalize(&task_path).expect("task path should canonicalize");
    let task_error = run_path(&task_path).expect_err("cross-module child should trap");
    assert_eq!(
        task_error.call_frames[0].span.path.as_deref(),
        Some(canonical_worker_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        task_error.task_ancestry[0].task_entry_span.path.as_deref(),
        Some(canonical_worker_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        task_error.task_ancestry[0].spawn_span.path.as_deref(),
        Some(canonical_task_path.to_string_lossy().as_ref())
    );

    fs::remove_dir_all(&root).expect("temporary module tree should be removable");
}

#[test]
fn recursion_depth_rejection_does_not_add_the_unentered_callee_frame() {
    let error = run_source(RECURSION_TRAP).expect_err("unbounded recursion should trap");
    assert!(
        error.message.contains("maximum call depth of 256 exceeded"),
        "unexpected recursion diagnostic: {error:?}"
    );
    assert_eq!(error.call_frames.len(), 256);
    assert_eq!(error.call_frames.first().unwrap().function, "recurse");
    assert_eq!(error.call_frames.last().unwrap().function, "main");
    assert_eq!(
        error
            .call_frames
            .iter()
            .filter(|frame| frame.function == "recurse")
            .count(),
        255,
        "the rejected attempted recurse call must not be recorded as active"
    );
}

#[test]
fn body_trap_captures_active_frames_before_task_group_cleanup() {
    let error = run_source(BODY_TRAP_DURING_TASK_GROUP_CLEANUP)
        .expect_err("the function-body bounds trap should remain primary");
    assert!(
        error
            .message
            .contains("vector index `9` is out of bounds for length `1`"),
        "unexpected primary diagnostic: {error:?}"
    );
    assert_eq!(
        error
            .call_frames
            .iter()
            .map(|frame| frame.function.as_str())
            .collect::<Vec<_>>(),
        vec!["main"],
        "cleanup must not replace or append to the body trap snapshot"
    );
    assert!(error.task_ancestry.is_empty());
}

#[test]
fn cleanup_primary_trap_captures_cleanup_function_and_active_caller_once() {
    let error =
        run_source(CLEANUP_PRIMARY_TRAP).expect_err("resource cleanup should establish the trap");
    assert!(
        error
            .message
            .contains("vector index `9` is out of bounds for length `1`"),
        "unexpected cleanup diagnostic: {error:?}"
    );
    assert_eq!(
        error
            .call_frames
            .iter()
            .map(|frame| frame.function.as_str())
            .collect::<Vec<_>>(),
        vec!["Resource.close", "main"],
        "the cleanup function and its active caller should be captured exactly once"
    );
    assert!(error.notes.iter().all(|note| !note.starts_with("Aura")));
    let rendered = error.render_with_source("cleanup.au", CLEANUP_PRIMARY_TRAP);
    assert_eq!(rendered.matches("Aura call chain").count(), 1);
}
