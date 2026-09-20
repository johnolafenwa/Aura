//! Allocation measurements for the Batch 1 representation phase (checkpoint
//! Q9 A, Q15 A, Q16 A). Every program runs on both backends with
//! `AURA_RUNTIME_STATS=1`; the direct backend's union counts pin the ratified
//! inline layout, and the interpreter's counts are reference numbers.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn write(&self, relative: &str, source: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::write(&path, source).expect("failed to write source");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stats {
    union_payload_boxes: u64,
    closure_environments: u64,
    opaque_boxes: u64,
    callable_overflow_allocations: u64,
}

fn parse_stats(stderr: &str, backend: &str) -> Stats {
    let prefix = format!("aura runtime stats ({backend}): ");
    let line = stderr
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no {backend} stats line in stderr:\n{stderr}"));
    let mut stats = Stats::default();
    for field in line[prefix.len()..].split_whitespace() {
        let (name, value) = field.split_once('=').expect("stats field");
        let value: u64 = value.parse().expect("stats value");
        match name {
            "union_payload_boxes" => stats.union_payload_boxes = value,
            "closure_environments" => stats.closure_environments = value,
            "opaque_boxes" => stats.opaque_boxes = value,
            "callable_overflow_allocations" => stats.callable_overflow_allocations = value,
            other => panic!("unexpected stats field `{other}`"),
        }
    }
    stats
}

fn run_with_stats(prefix: &str, source: &str, backend: &str) -> (String, Stats) {
    let temp = TempDir::new(prefix);
    let path = temp.write("main.au", source);
    let output = Command::new(aura_bin())
        .arg("run")
        .arg("--backend")
        .arg(backend)
        .arg(&path)
        .env("AURA_RUNTIME_STATS", "1")
        .output()
        .expect("failed to run aura");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "{backend} run should succeed, stderr was:\n{stderr}"
    );
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        parse_stats(&stderr, backend),
    )
}

fn assert_both_backends(prefix: &str, source: &str, expected_stdout: &str) -> (Stats, Stats) {
    let (mir_stdout, mir) = run_with_stats(prefix, source, "mir");
    let (direct_stdout, direct) = run_with_stats(prefix, source, "direct");
    assert_eq!(mir_stdout, expected_stdout, "interpreter stdout");
    assert_eq!(direct_stdout, expected_stdout, "direct stdout");
    (mir, direct)
}

const SCALAR_UNION_LOCAL: &str = "def main():\n    value: int64 | None = 41\n    match value:\n        case int64 as number:\n            print(number + 1)\n        case None:\n            print(\"none\")\n";

const CLASS_UNION_LOCAL: &str = "class Point:\n    x: int64\n    y: int64\n\ndef main():\n    slot: Point | None = Point(x=3, y=4)\n    match slot:\n        case Point as point:\n            print(point.x + point.y)\n        case None:\n            print(\"none\")\n";

const UNION_CLASS_FIELD: &str = "class Slot:\n    value: int64 | None\n\ndef main():\n    slot = Slot(value=None)\n    match slot.value:\n        case int64 as number:\n            print(number)\n        case None:\n            print(\"empty\")\n    filled = Slot(value=7)\n    match filled.value:\n        case int64 as number:\n            print(number)\n        case None:\n            print(\"empty\")\n";

#[test]
fn stats_report_appears_only_when_requested_on_both_backends() {
    let temp = TempDir::new("repr-stats-shape");
    let path = temp.write("main.au", SCALAR_UNION_LOCAL);
    for backend in ["mir", "direct"] {
        let silent = Command::new(aura_bin())
            .args(["run", "--backend", backend])
            .arg(&path)
            .env_remove("AURA_RUNTIME_STATS")
            .output()
            .expect("failed to run aura");
        assert!(silent.status.success());
        assert!(
            !String::from_utf8_lossy(&silent.stderr).contains("aura runtime stats"),
            "{backend} must not report stats without the request"
        );
        let (_, stats) = run_with_stats("repr-stats-shape", SCALAR_UNION_LOCAL, backend);
        assert_eq!(stats.callable_overflow_allocations, 0);
        assert_eq!(stats.closure_environments, 0, "{backend} packs no closure here");
    }
}

#[test]
fn direct_scalar_union_local_allocates_no_union_box() {
    let (mir, direct) = assert_both_backends("repr-scalar-union", SCALAR_UNION_LOCAL, "42\n");
    assert!(mir.union_payload_boxes >= 1, "the interpreter boxes its union payload");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "an `int64 | None` local stays inline on the direct backend (Q9 A)"
    );
}

#[test]
fn direct_class_union_local_allocates_no_union_box() {
    let (_, direct) = assert_both_backends("repr-class-union", CLASS_UNION_LOCAL, "7\n");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "a `Point | None` local stays inline on the direct backend (Q9 A)"
    );
}

#[test]
fn direct_union_class_field_allocates_no_union_box() {
    let (_, direct) = assert_both_backends("repr-union-field", UNION_CLASS_FIELD, "empty\n7\n");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "a union class field is flattened into the plain class layout (Q9 A)"
    );
}
