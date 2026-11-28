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
    _ = ready.put(marker)
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
    _ = ready.put(marker)
    sleep(5s)
    match parsed:
        case Result.Ok(other):
            return scene.points.len() + other.points.len()
        case Result.Err(_):
            return -4

def fail_holding(ready: Queue[int64], marker: int64) -> int64:
    scene = build(marker)
    _ = ready.put(marker)
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

/// Enum, union, and class fields of a class lie in boxes of their own,
/// payload boxes move out of typed variants, and records pop off a list;
/// the native run frees every box.
#[test]
fn class_fields_in_boxes_free_every_box() {
    let source =
        include_str!("../../aura-compiler/tests/fixtures/run-pass/class_fields_in_boxes.au");
    let expected =
        include_str!("../../aura-compiler/tests/fixtures/run-pass/class_fields_in_boxes.stdout");
    assert_frees_everything("leak-class-field-boxes", source, expected, 0);
}

/// Lists that keep new instances in place, read, copied, edited, sorted,
/// and sent to a task: the native run frees every box.
#[test]
fn instance_lists_in_place_free_every_box() {
    let source =
        include_str!("../../aura-compiler/tests/fixtures/run-pass/instance_lists_in_place.au");
    let expected =
        include_str!("../../aura-compiler/tests/fixtures/run-pass/instance_lists_in_place.stdout");
    assert_frees_everything("leak-instance-lists", source, expected, 0);
}

/// Elements a list keeps in place, passed to functions that read only
/// their scalar fields, by a key sort and by calls: the native run frees
/// every box.
#[test]
fn element_args_read_in_place_free_every_box() {
    let source =
        include_str!("../../aura-compiler/tests/fixtures/run-pass/element_args_read_in_place.au");
    let expected = include_str!(
        "../../aura-compiler/tests/fixtures/run-pass/element_args_read_in_place.stdout"
    );
    assert_frees_everything("leak-element-args", source, expected, 0);
}

/// Payloads that entry loans read as their keys where the variant holds
/// them, with the statements changed and cleared afterwards and a missing
/// key that traps in a task: the native run frees every box.
#[test]
fn payload_keys_lent_to_entry_loans_free_every_box() {
    let source = include_str!(
        "../../aura-compiler/tests/fixtures/run-pass/payload_keys_lent_to_entry_loans.au"
    );
    let expected = include_str!(
        "../../aura-compiler/tests/fixtures/run-pass/payload_keys_lent_to_entry_loans.stdout"
    );
    assert_frees_everything("leak-payload-keys", source, expected, 0);
}

/// F-strings built in the box of a text they read for the last time, with
/// texts other places still hold and literals: the native run frees every
/// box.
#[test]
fn fstrings_built_in_a_last_read_str_free_every_box() {
    let source = include_str!(
        "../../aura-compiler/tests/fixtures/run-pass/fstring_builds_in_a_last_read_str.au"
    );
    let expected = include_str!(
        "../../aura-compiler/tests/fixtures/run-pass/fstring_builds_in_a_last_read_str.stdout"
    );
    assert_frees_everything("leak-fstring-last-read", source, expected, 0);
}

/// An overflow in a function that reads an element a list keeps in place
/// where it lies, called by a loop and by a key sort, names both frames
/// and frees every box.
#[test]
fn an_overflow_reading_an_element_in_place_names_both_frames() {
    let source = r#"class Cell:
    id: int64
    big: int64

def weight(cell: Cell) -> int64:
    return cell.big * 3 + cell.id

def main() -> None:
    mut cells: list[Cell] = []
    for i in range(40):
        cells.append(Cell(id=i, big=i))
    cells.append(Cell(id=40, big=4611686018427387904))
    SORT
    mut total = 0
    for i in range(cells.len()):
        total += weight(cells[i])
    print(total)
"#;
    for sort in ["pass", "cells.sort(key=weight)"] {
        let source = source.replace("SORT", sort);
        for backend in ["mir", "direct"] {
            let result = run("element-in-place-overflow", &source, backend);
            assert_eq!(result.code, Some(1), "{}", result.stderr);
            assert!(result.stdout.is_empty(), "{}", result.stdout);
            assert!(result.stderr.contains("AU4002"), "{}", result.stderr);
            assert!(
                result.stderr.contains("weight at 5:1 -> main at 8:1"),
                "{}",
                result.stderr
            );
            if backend == "direct" {
                let (allocated, freed) = result.boxes.expect("direct runtime stats");
                assert_eq!(allocated, freed, "{}", result.stderr);
            }
        }
    }
}

/// A caller that defers its frame record calls a function that only reads
/// variants and compares them without pushing the record. A trap in the
/// caller after such a call, or in a callee that may trap, still names
/// every frame, and the native run frees every box.
#[test]
fn a_trap_after_a_frame_free_call_names_every_frame() {
    let source = r#"enum Value:
    Int(value: int64)
    Text(value: str)

def same(left: Value, right: Value) -> bool:
    match left:
        case Value.Int(a):
            match right:
                case Value.Int(b):
                    return a == b
                case Value.Text(_):
                    return false
        case Value.Text(a):
            match right:
                case Value.Text(b):
                    return a == b
                case Value.Int(_):
                    return false

def scaled(value: Value, factor: int64) -> int64:
    match value:
        case Value.Int(a):
            return a * factor
        case Value.Text(_):
            return 0

def count(values: list[Value], probe: Value, limit: int64) -> int64:
    mut found = 0
    for value in values:
        if same(value, probe):
            found += ADD
    return found

def main() -> None:
    values = [Value.Int(1), Value.Text("a"), Value.Int(1), Value.Int(2)]
    print(count(values, Value.Int(1), 3))
    print(count(values, Value.Text("a"), 9223372036854775807))
    print(count(values, Value.Int(2), 4611686018427387904))
    print(count(values, Value.Int(1), 4611686018427387904))
"#;
    for (add, stdout, chain) in [
        (
            "limit",
            "6\n9223372036854775807\n4611686018427387904\n",
            "count at 27:1 -> main at 34:1",
        ),
        (
            "scaled(value, limit)",
            "6\n0\n",
            "scaled at 20:1 -> count at 27:1 -> main at 34:1",
        ),
    ] {
        let source = source.replace("ADD", add);
        for backend in ["mir", "direct"] {
            let result = run("frame-free-call-trap", &source, backend);
            assert_eq!(result.code, Some(1), "{}", result.stderr);
            assert_eq!(result.stdout, stdout, "{}", result.stderr);
            assert!(result.stderr.contains("AU4002"), "{}", result.stderr);
            assert!(result.stderr.contains(chain), "{}", result.stderr);
            if backend == "direct" {
                let (allocated, freed) = result.boxes.expect("direct runtime stats");
                assert_eq!(allocated, freed, "{}", result.stderr);
            }
        }
    }
}

/// A caller that defers its frame record lends a function that can only
/// trap a link naming itself instead of pushing the record, and such a
/// function lends one to its own callees. A trap in the innermost callee
/// still names every frame, whether or not the caller had pushed its
/// record by then, and the native run frees every box.
#[test]
fn a_trap_in_a_chain_callee_names_every_frame() {
    let source = r#"copy class Stamp:
    seconds: int64
    offset: int64

    def local(self) -> int64:
        return self.seconds + self.offset * 60

    def days(self) -> int64:
        return self.local() // 86400

def era_day(days: int64) -> int64:
    shifted = days + 719468
    return shifted - shifted // 146097 * 146097

def year_of(days: int64) -> int64:
    day = era_day(days)
    return (day - day // 1460 + day // 36524) // 365

def describe(stamp: Stamp, label: str) -> str:
    PUSH
    return f"{label} {year_of(stamp.days())} {year_of(stamp.seconds)}"

def main() -> None:
    names = ["a", "b"]
    print(describe(Stamp(seconds=1700000000, offset=60), names[0]))
    print(describe(Stamp(seconds=SECONDS, offset=OFFSET), names[1]))
"#;
    for (push, seconds, offset, chain) in [
        (
            "pass",
            "9223372036854775000",
            "0",
            "era_day at 11:1 -> year_of at 15:1 -> describe at 19:1 -> main at 23:1",
        ),
        (
            "print(label)",
            "9223372036854775000",
            "0",
            "era_day at 11:1 -> year_of at 15:1 -> describe at 19:1 -> main at 23:1",
        ),
        (
            "pass",
            "9223372036854775000",
            "100000000",
            "Stamp.local at 5:5 -> Stamp.days at 8:5 -> describe at 19:1 -> main at 23:1",
        ),
    ] {
        let source = source
            .replace("PUSH", push)
            .replace("SECONDS", seconds)
            .replace("OFFSET", offset);
        let mut outputs = Vec::new();
        for backend in ["mir", "direct"] {
            let result = run("chain-callee-trap", &source, backend);
            assert_eq!(result.code, Some(1), "{}", result.stderr);
            assert!(result.stderr.contains("AU4002"), "{}", result.stderr);
            assert!(result.stderr.contains(chain), "{}", result.stderr);
            if backend == "direct" {
                let (allocated, freed) = result.boxes.expect("direct runtime stats");
                assert_eq!(allocated, freed, "{}", result.stderr);
            }
            outputs.push(result.stdout);
        }
        assert_eq!(outputs[0], outputs[1]);
    }
}

#[test]
fn a_decorated_module_function_owns_its_captures_until_shutdown() {
    let source = r#"
type Step = Callable[def(value: int64) -> int64]
type Decorator = Callable[def(step: own Step) -> Step]

def named(label: own str) -> Decorator:
    return lambda [own label] own step: named_step(label.clone(), step)

def named_step(label: own str, step: own Step) -> Step:
    return lambda [own label, own step] value: label.len() + step(value)

@named("tag")
def twice(value: int64) -> int64:
    return value * 2

def main() -> None:
    print(twice(4))
"#;
    assert_frees_everything("leak-decorator-captures", source, "11\n", 0);
}

#[test]
fn an_overflow_in_a_forwarded_method_names_both_frames() {
    let source = r#"type Tally = Callable[mut def(amount: int64) -> int64]

class Count:
    value: int64

    def add(mut self, amount: int64) -> int64:
        self.value += amount
        return self.value

def counter(start: int64) -> Tally:
    count = Count(value=start)
    return lambda [own count] amount: count.add(amount)

def main() -> None:
    mut tally = counter(9223372036854775807)
    print(tally(1))
"#;
    for backend in ["mir", "direct"] {
        let result = run("forwarded-method-overflow", source, backend);
        assert_eq!(result.code, Some(1), "{}", result.stderr);
        assert!(result.stdout.is_empty(), "{}", result.stdout);
        assert!(result.stderr.contains("AU4002"), "{}", result.stderr);
        assert!(
            result
                .stderr
                .contains("Count.add at 6:5 -> counter::__lambda_12_12 at 12:12 -> main at 14:1"),
            "{}",
            result.stderr
        );
        if backend == "direct" {
            // The scalar receiver can stay entirely in the callable's words.
            let (allocated, freed) = result.boxes.expect("direct runtime stats");
            assert_eq!(allocated, freed, "{}", result.stderr);
        }
    }
}

#[test]
fn trait_dispatch_preserves_returned_unions_and_callables() {
    let source = r#"
trait Producer:
    def label(self) -> str | None
    def items(self) -> list[int64] | None
    def counter(self) -> Callable[def() -> int64]

class Alpha:
    prefix: str
    base: int64

impl Producer for Alpha:
    def label(self) -> str | None:
        return f"{self.prefix}-alpha-{self.base}"

    def items(self) -> list[int64] | None:
        return [self.base, self.base + 1, self.base + 2]

    def counter(self) -> Callable[def() -> int64]:
        tag = f"{self.prefix}-{self.base}"
        offset = self.base
        return lambda [own tag, own offset]: tag.len() + offset

class Beta:
    base: int64

impl Producer for Beta:
    def label(self) -> str | None:
        return None

    def items(self) -> list[int64] | None:
        return [self.base * 10]

    def counter(self) -> Callable[def() -> int64]:
        offset = self.base
        return lambda [own offset]: offset * 2

def inspect(p: dyn Producer) -> str:
    text = match own p.label():
        case str as s:
            s
        case None:
            "none"
    mut total = 0
    match p.items():
        case list[int64] as xs:
            for x in xs:
                total += x
        case None:
            total = -1
    cb = p.counter()
    return f"{text}|{total}|{cb()}"

def inspect_generic[T: Producer](p: T) -> str:
    text = match own p.label():
        case str as s:
            s
        case None:
            "none"
    mut total = 0
    match p.items():
        case list[int64] as xs:
            for x in xs:
                total += x
        case None:
            total = -1
    cb = p.counter()
    return f"{text}|{total}|{cb()}"

def main() -> None:
    a: dyn Producer = Alpha(prefix="hello", base=10)
    b: dyn Producer = Beta(base=5)
    print(inspect(a))
    print(inspect(b))
    print(inspect_generic(Alpha(prefix="world", base=3)))
    print(inspect_generic(Beta(base=4)))
"#;
    assert_frees_everything(
        "trait-dispatch-union-callable",
        source,
        "hello-alpha-10|33|18\nnone|50|10\nworld-alpha-3|12|10\nnone|40|8\n",
        0,
    );
}
