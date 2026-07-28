use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

fn write_source(name: &str, source: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should follow the Unix epoch")
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("aurora-{name}-{}-{unique}.au", std::process::id()));
    fs::write(&path, source).expect("temporary Aurora source should write");
    path
}

fn assert_mir_run_stderr_contains(name: &str, source: &str, expected: &[&str]) {
    let path = write_source(name, source);
    let output = Command::new(aura_bin())
        .arg("run")
        .arg(&path)
        .output()
        .expect("aura run should start");
    let _ = fs::remove_file(&path);
    assert!(
        !output.status.success(),
        "source should trap under MIR execution"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in expected {
        assert!(
            stderr.contains(expected),
            "MIR human diagnostic should contain `{expected}`; stderr was:\n{stderr}"
        );
    }
}

fn run_json_path(path: &std::path::Path, backend: &str) -> serde_json::Value {
    let output = Command::new(aura_bin())
        .args(["run", "--format", "json", "--backend", backend])
        .arg(path)
        .output()
        .unwrap_or_else(|error| panic!("{backend} JSON run should start: {error}"));
    assert_eq!(
        output.status.code(),
        Some(1),
        "{backend} source should trap; stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.lines().count(),
        1,
        "{backend} JSON trap must be exactly one document: {stderr}"
    );
    serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
        panic!("{backend} JSON trap should parse: {error}; stderr was:\n{stderr}")
    })
}

fn point_span(path: &std::path::Path, line: usize, column: usize) -> serde_json::Value {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    serde_json::json!({
        "path": path.display().to_string(),
        "start": {"line": line, "column": column},
        "end": {"line": line, "column": column + 1},
    })
}

fn run_json_stdin(virtual_path: &str, source: &str, backend: &str) -> serde_json::Value {
    let mut child = Command::new(aura_bin())
        .args([
            "run",
            "--format",
            "json",
            "--backend",
            backend,
            "--stdin",
            virtual_path,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{backend} stdin JSON run should start: {error}"));
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("virtual source should write");
    let output = child
        .wait_with_output()
        .expect("stdin JSON run should remain waitable");
    assert_eq!(
        output.status.code(),
        Some(1),
        "{backend} virtual source should trap; stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
        panic!(
            "{backend} virtual-path trap should be one JSON document: {error}; stderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn aura_run_renders_nested_call_and_child_task_backtraces() {
    let nested = "def explode() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef relay() -> int32:\n    return explode()\n\ndef main() -> int32:\n    return relay()\n";
    assert_mir_run_stderr_contains(
        "nested-call-backtrace",
        nested,
        &["note: Aurora call chain (innermost first): explode at 1:1 -> relay at 5:1 -> main at 8:1"],
    );

    let task = "def child() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef main() -> int32:\n    with group = TaskGroup():\n        group.start(child)\n    return 0\n";
    assert_mir_run_stderr_contains(
        "child-task-backtrace",
        task,
        &[
            "note: Aurora call chain (innermost first): child at 1:1",
            "note: Aurora task entry: child at 1:1",
            "note: Aurora task ancestry (youngest first): child spawned from main at 7:15",
        ],
    );
}

#[test]
fn standalone_direct_human_trap_keeps_frames_and_runs_cleanup_once() {
    let source = "class Resource:\n    def close(mut self):\n        print(\"CLEANUP\")\n\ndef child() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef main() -> int32:\n    with resource = Resource():\n        with group = TaskGroup():\n            group.start(child)\n    return 0\n";
    let path = write_source("standalone-human-task-cleanup", source);
    let binary = path.with_extension("bin");
    let build = Command::new(aura_bin())
        .args(["build", "--backend", "direct", "-o"])
        .arg(&binary)
        .arg(&path)
        .output()
        .expect("standalone direct build should start");
    assert!(
        build.status.success(),
        "standalone direct build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&binary)
        .output()
        .expect("standalone direct binary should start");
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "CLEANUP\n",
        "the body trap must run the outer cleanup exactly once"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    for expected in [
        "note: Aurora call chain (innermost first): child at 5:1",
        "note: Aurora task entry: child at 5:1",
        "note: Aurora task ancestry (youngest first): child spawned from main at 12:19",
    ] {
        assert!(
            stderr.contains(expected),
            "standalone human diagnostic must contain `{expected}`; stderr was:\n{stderr}"
        );
    }
    assert!(
        stderr.contains("vector index `9` is out of bounds"),
        "cleanup must not replace the original body trap: {stderr}"
    );
    let _ = fs::remove_file(binary);
    let _ = fs::remove_file(path);
}

#[test]
fn mir_and_direct_json_use_the_same_typed_call_and_task_frames() {
    let nested = "def explode() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef relay() -> int32:\n    return explode()\n\ndef main() -> int32:\n    return relay()\n";
    let nested_path = write_source("nested-json-parity", nested);
    let nested_mir = run_json_path(&nested_path, "mir");
    let nested_direct = run_json_path(&nested_path, "direct");
    let expected_nested_calls = serde_json::json!([
        {"function": "explode", "span": point_span(&nested_path, 1, 1)},
        {"function": "relay", "span": point_span(&nested_path, 5, 1)},
        {"function": "main", "span": point_span(&nested_path, 8, 1)},
    ]);
    for report in [&nested_mir, &nested_direct] {
        let diagnostic = &report["diagnostics"][0];
        assert_eq!(diagnostic["code"], "AU4003");
        assert_eq!(
            diagnostic["call_frames"], expected_nested_calls,
            "typed call frames must preserve every function and source coordinate"
        );
        assert_eq!(diagnostic["task_ancestry"], serde_json::json!([]));
        assert!(
            diagnostic["notes"]
                .as_array()
                .is_some_and(|notes| notes.iter().all(|note| !note
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("Aurora call chain"))),
            "generated frame prose must be absent from structured notes: {diagnostic}"
        );
    }
    assert_eq!(
        nested_mir["diagnostics"][0]["call_frames"],
        nested_direct["diagnostics"][0]["call_frames"]
    );
    assert_eq!(
        nested_mir["diagnostics"][0]["task_ancestry"],
        nested_direct["diagnostics"][0]["task_ancestry"]
    );
    let _ = fs::remove_file(&nested_path);

    let task = "def child() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef main() -> int32:\n    with group = TaskGroup():\n        group.start(child)\n    return 0\n";
    let task_path = write_source("task-json-parity", task);
    let task_mir = run_json_path(&task_path, "mir");
    let task_direct = run_json_path(&task_path, "direct");
    let expected_task_calls =
        serde_json::json!([{"function": "child", "span": point_span(&task_path, 1, 1)}]);
    let expected_task_ancestry = serde_json::json!([{
        "task_function": "child",
        "task_entry_span": point_span(&task_path, 1, 1),
        "parent_function": "main",
        "spawn_span": point_span(&task_path, 7, 15),
    }]);
    for report in [&task_mir, &task_direct] {
        let diagnostic = &report["diagnostics"][0];
        assert_eq!(
            diagnostic["call_frames"], expected_task_calls,
            "task call frames must preserve their complete source span"
        );
        assert_eq!(
            diagnostic["task_ancestry"], expected_task_ancestry,
            "task ancestry must preserve complete entry and spawn spans"
        );
    }
    assert_eq!(
        task_mir["diagnostics"][0]["call_frames"],
        task_direct["diagnostics"][0]["call_frames"]
    );
    assert_eq!(
        task_mir["diagnostics"][0]["task_ancestry"],
        task_direct["diagnostics"][0]["task_ancestry"]
    );
    let _ = fs::remove_file(&task_path);
}

#[test]
fn mir_and_direct_json_preserve_exact_nested_task_ancestry() {
    let source = "def grandchild() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef child() -> int32:\n    with group = TaskGroup():\n        group.start(grandchild)\n    return 0\n\ndef main() -> int32:\n    with group = TaskGroup():\n        group.start(child)\n    return 0\n";
    let path = write_source("nested-task-json-parity", source);
    let mir = run_json_path(&path, "mir");
    let direct = run_json_path(&path, "direct");
    let expected_calls =
        serde_json::json!([{"function": "grandchild", "span": point_span(&path, 1, 1)}]);
    let expected_ancestry = serde_json::json!([
        {
            "task_function": "grandchild",
            "task_entry_span": point_span(&path, 1, 1),
            "parent_function": "child",
            "spawn_span": point_span(&path, 7, 15),
        },
        {
            "task_function": "child",
            "task_entry_span": point_span(&path, 5, 1),
            "parent_function": "main",
            "spawn_span": point_span(&path, 12, 15),
        },
    ]);
    for report in [&mir, &direct] {
        assert_eq!(report["diagnostics"][0]["call_frames"], expected_calls);
        assert_eq!(report["diagnostics"][0]["task_ancestry"], expected_ancestry);
    }
    assert_eq!(
        mir["diagnostics"][0]["call_frames"],
        direct["diagnostics"][0]["call_frames"]
    );
    assert_eq!(
        mir["diagnostics"][0]["task_ancestry"],
        direct["diagnostics"][0]["task_ancestry"]
    );
    let _ = fs::remove_file(path);
}

#[test]
fn direct_json_stdin_preserves_the_virtual_path_in_typed_frames() {
    let virtual_path = "virtual/transport/nested.au";
    let source = "def explode() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n\ndef main() -> int32:\n    return explode()\n";
    let mir = run_json_stdin(virtual_path, source, "mir");
    let direct = run_json_stdin(virtual_path, source, "direct");
    let resolved_path = std::env::current_dir()
        .expect("current directory should resolve")
        .join(virtual_path);
    let expected = serde_json::json!([
        {"function": "explode", "span": point_span(&resolved_path, 1, 1)},
        {"function": "main", "span": point_span(&resolved_path, 5, 1)},
    ]);
    assert_eq!(mir["diagnostics"][0]["call_frames"], expected);
    assert_eq!(direct["diagnostics"][0]["call_frames"], expected);
    assert_eq!(
        mir["diagnostics"][0]["call_frames"],
        direct["diagnostics"][0]["call_frames"]
    );
    assert_eq!(
        mir["diagnostics"][0]["task_ancestry"],
        direct["diagnostics"][0]["task_ancestry"]
    );
}

#[test]
fn structured_frames_keep_imported_function_paths_on_both_backends() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should follow the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aurora-imported-frame-{}-{unique}",
        std::process::id()
    ));
    let helpers = root.join("helpers");
    fs::create_dir_all(&helpers).expect("helper directory should be creatable");
    let helper_path = helpers.join("boom.au");
    let main_path = root.join("main.au");
    fs::write(
        &helper_path,
        "public def explode() -> int32:\n    values: Vec[int32] = [1, 2]\n    return values[9]\n",
    )
    .expect("helper source should write");
    fs::write(
        &main_path,
        "import helpers.boom\n\ndef main() -> int32:\n    with group = TaskGroup():\n        group.start(helpers.boom.explode)\n    return 0\n",
    )
    .expect("entry source should write");

    let expected_calls = serde_json::json!([
        {"function": "helpers.boom::explode", "span": point_span(&helper_path, 1, 8)}
    ]);
    let expected_ancestry = serde_json::json!([{
        "task_function": "helpers.boom::explode",
        "task_entry_span": point_span(&helper_path, 1, 8),
        "parent_function": "main",
        "spawn_span": point_span(&main_path, 5, 15),
    }]);
    let mut reports = Vec::new();
    for backend in ["mir", "direct"] {
        let output = Command::new(aura_bin())
            .args(["run", "--format", "json", "--backend", backend])
            .arg(&main_path)
            .output()
            .unwrap_or_else(|error| panic!("{backend} imported-frame run failed: {error}"));
        assert_eq!(
            output.status.code(),
            Some(1),
            "{backend} imported source should trap; stderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value =
            serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
                panic!(
                    "{backend} imported trap should be one JSON document: {error}; stderr was:\n{}",
                    String::from_utf8_lossy(&output.stderr)
                )
            });
        assert_eq!(
            report["diagnostics"][0]["call_frames"], expected_calls,
            "{backend} must retain the imported task entry path and coordinates"
        );
        assert_eq!(
            report["diagnostics"][0]["task_ancestry"], expected_ancestry,
            "{backend} must retain the importing task spawn path and coordinates"
        );
        reports.push(report);
    }
    assert_eq!(
        reports[0]["diagnostics"][0]["call_frames"],
        reports[1]["diagnostics"][0]["call_frames"]
    );
    assert_eq!(
        reports[0]["diagnostics"][0]["task_ancestry"],
        reports[1]["diagnostics"][0]["task_ancestry"]
    );
    let _ = fs::remove_dir_all(root);
}
