use std::fs;
use std::path::PathBuf;
use std::process::Command;
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
