//! Leak, failure, and cancellation cleanup (Rust-speed step 2h). A native
//! program run with `AURA_RUNTIME_STATS=1` reports how many boxes it
//! allocated (`opaque_boxes`) and freed (`freed_boxes`) once its module
//! constants are gone. Typed layouts keep every heap block inside a box,
//! so a program that frees every box frees every block too. Each program
//! here holds records in lists, instance fields, typed variants, and typed
//! `Result` values across frames and tasks when it finishes, traps, or
//! cancels, and must end with nothing left.

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
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// One run's standard output, exit code, and the direct runtime's
/// allocated and freed box counts.
struct Run {
    stdout: String,
    stderr: String,
    code: Option<i32>,
    boxes: Option<(u64, u64)>,
}

fn run(prefix: &str, source: &str, backend: &str) -> Run {
    let temp = TempDir::new(prefix);
    let path = temp.path.join("main.au");
    fs::write(&path, source).expect("failed to write source");
    let output = Command::new(aura_bin())
        .args(["run", "--backend", backend])
        .arg(&path)
        .env("AURA_RUNTIME_STATS", "1")
        .output()
        .expect("failed to run aura");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let prefix = format!("aura runtime stats ({backend}): ");
    let boxes = stderr
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
        .map(|fields| {
            let field = |name: &str| {
                fields
                    .split_whitespace()
                    .find_map(|field| field.strip_prefix(name)?.strip_prefix('='))
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or_else(|| panic!("no `{name}` in the stats line: {fields}"))
            };
            (field("opaque_boxes"), field("freed_boxes"))
        });
    Run {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr,
        code: output.status.code(),
        boxes,
    }
}

/// Runs `source` on both backends, checks they agree with `expected_stdout`
/// and `expected_code`, and that the native build freed every box it
/// allocated. Returns the native run's standard error.
fn assert_frees_everything(
    prefix: &str,
    source: &str,
    expected_stdout: &str,
    expected_code: i32,
) -> String {
    let interpreted = run(prefix, source, "mir");
    assert_eq!(
        (interpreted.stdout.as_str(), interpreted.code),
        (expected_stdout, Some(expected_code)),
        "interpreter run, stderr was:\n{}",
        interpreted.stderr
    );
    let native = run(prefix, source, "direct");
    assert_eq!(
        (native.stdout.as_str(), native.code),
        (expected_stdout, Some(expected_code)),
        "native run, stderr was:\n{}",
        native.stderr
    );
    let (allocated, freed) = native
        .boxes
        .unwrap_or_else(|| panic!("no direct stats line in stderr:\n{}", native.stderr));
    assert!(allocated > 0, "the program should allocate boxes");
    assert_eq!(
        freed,
        allocated,
        "the native program left {} of {allocated} boxes, stderr was:\n{}",
        allocated - freed,
        native.stderr
    );
    native.stderr
}

const TYPES: &str = r#"
copy class Point:
    x: int64
    y: int64

class Scene:
    name: str
    points: list[Point]
    labels: dict[str, list[Point]]

enum Shape:
    Dot(Point)
    Path(list[Point])
    Named(str, Shape)

def build(marker: int64) -> Scene:
    points = [Point(x=marker, y=marker + 1), Point(x=2, y=3)]
    return Scene(name=f"scene {marker}", points=points.copy(), labels={"p": points})

def parse(text: str) -> Result[int64, str]:
    if text == "bad":
        return Result.Err(f"cannot parse {text}")
    return Result.Ok(text.len())

def area(shape: Shape) -> int64:
    match shape:
        case Shape.Dot(point):
            return point.x
        case Shape.Path(points):
            return points.len()
        case Shape.Named(_, inner):
            return area(inner)

def descend(scene: Scene, shapes: list[Shape], depth: int64, stop: int64) -> int64:
    local = Point(x=depth, y=depth * 2)
    mut trail = [local, Point(x=1, y=2)]
    named = Shape.Named(f"level {depth}", Shape.Path(trail.copy()))
    parsed = parse(scene.name)
    trail.append(local)
    if depth == 0:
        values = [area(named), shapes.len()]
        match parsed:
            case Result.Ok(size):
                return values[size - stop]
            case Result.Err(message):
                return message.len()
    return descend(scene, shapes, depth - 1, stop) + area(named) + trail.len()
"#;

#[test]
fn a_finished_program_frees_every_box() {
    let source = format!(
        "{TYPES}
def main() -> int32:
    scene = build(1)
    shapes = [Shape.Dot(Point(x=3, y=4)), Shape.Named(\"n\", Shape.Path(scene.points.copy()))]
    print(descend(scene, shapes, 4, 7))
    return 0
"
    );
    assert_frees_everything("leak-finished", &source, "22\n", 0);
}

#[test]
fn a_trap_deep_in_calls_frees_every_frame_s_boxes() {
    let source = format!(
        "{TYPES}
def main() -> int32:
    scene = build(1)
    shapes = [Shape.Dot(Point(x=3, y=4)), Shape.Named(\"n\", Shape.Path(scene.points.copy()))]
    print(descend(scene, shapes, 4, 0))
    return 0
"
    );
    let stderr = assert_frees_everything("leak-trap", &source, "", 1);
    assert!(
        stderr.contains("AU4003"),
        "the program should stop at the list index, stderr was:\n{stderr}"
    );
}

const TASKS: &str = r#"
def wait_holding(ready: Queue[int64], gate: Queue[int64], marker: int64) -> int64:
    scene = build(marker)
    shape = Shape.Named(scene.name.clone(), Shape.Path(scene.points.copy()))
    ready.put(marker)
    match gate.get(timeout=5s):
        case QueueReceive.Item(value):
            return value
        case QueueReceive.Closed:
            return -1
        case QueueReceive.TimedOut:
            return -2
        case QueueReceive.Cancelled:
            match shape:
                case Shape.Named(name, _):
                    return name.len() + scene.points.len()
                case _:
                    return -3

def sleep_holding(ready: Queue[int64], marker: int64) -> int64:
    scene = build(marker)
    parsed: Result[Scene, str] = Result.Ok(build(marker + 1))
    ready.put(marker)
    sleep(5s)
    match parsed:
        case Result.Ok(other):
            return scene.points.len() + other.points.len()
        case Result.Err(_):
            return -4

def fail_holding(ready: Queue[int64], marker: int64) -> int64:
    scene = build(marker)
    ready.put(marker)
    shape = Shape.Path(scene.points.copy())
    assert scene.points.len() == 5, "task failure with values alive"
    return area(shape)

def outcome(task: Task[int64]) -> str:
    match task.result(timeout=5s):
        case TaskResult.Ready(value):
            return f"ready {value}"
        case TaskResult.Error(_):
            return "error"
        case TaskResult.TimedOut:
            return "timed out"
        case TaskResult.Cancelled:
            return "cancelled"

def wait_ready(ready: Queue[int64], count: int64) -> bool:
    for _ in range(count):
        match ready.get(timeout=5s):
            case QueueReceive.Item(_):
                pass
            case _:
                return false
    return true
"#;

#[test]
fn failed_and_cancelled_tasks_free_every_box() {
    let source = format!(
        "{TYPES}{TASKS}
def main() -> int32:
    ready = Queue[int64]()
    gate = Queue[int64]()
    with TaskGroup() as group:
        waiting = group.start(wait_holding, ready, gate, 1)
        sleeping = group.start(sleep_holding, ready, 2)
        failing = group.start(fail_holding, ready, 3)
        if not wait_ready(ready, 3):
            return 1
        print(outcome(failing))
        group.cancel()
        print(outcome(waiting))
        print(outcome(sleeping))
    return 0
"
    );
    assert_frees_everything("leak-tasks", &source, "error\nready 9\nready 4\n", 0);
}

#[test]
fn a_trap_while_tasks_hold_values_frees_every_box() {
    let source = format!(
        "{TYPES}{TASKS}
def main() -> int32:
    ready = Queue[int64]()
    gate = Queue[int64]()
    scene = build(9)
    with TaskGroup() as group:
        waiting = group.start(wait_holding, ready, gate, 1)
        sleeping = group.start(sleep_holding, ready, 2)
        if not wait_ready(ready, 2):
            return 1
        values = [scene.points.len()]
        print(values[scene.points.len()])
        print(outcome(waiting))
        print(outcome(sleeping))
    return 0
"
    );
    let stderr = assert_frees_everything("leak-task-trap", &source, "", 1);
    assert!(
        stderr.contains("AU4003"),
        "the program should stop at the list index, stderr was:\n{stderr}"
    );
}

/// Typed values cross queues, task arguments, and task results: plain
/// records, tuples, classes with list and dict fields, enums with record
/// payloads, and `Result`, including the values `try_put` and a closed
/// queue's `put` hand back. The debug runtime these tests build checks
/// that no value reaches another thread without being shared.
#[test]
fn typed_values_crossing_tasks_and_queues_free_every_box() {
    let source =
        include_str!("../../aura-compiler/tests/fixtures/run-pass/typed_values_cross_tasks.au");
    let expected =
        include_str!("../../aura-compiler/tests/fixtures/run-pass/typed_values_cross_tasks.stdout");
    assert_frees_everything("leak-typed-transfers", source, expected, 0);
}

/// Module constants of every kind are read in place from many tasks,
/// copied, and changed through copies. Each constant is built once and
/// frozen, and the native run frees every box, constants included.
#[test]
fn module_constants_read_across_tasks_free_every_box() {
    let source = include_str!(
        "../../aura-compiler/tests/fixtures/run-pass/module_constants_read_across_tasks.au"
    );
    let expected = include_str!(
        "../../aura-compiler/tests/fixtures/run-pass/module_constants_read_across_tasks.stdout"
    );
    assert_frees_everything("leak-frozen-constants", source, expected, 0);
}
