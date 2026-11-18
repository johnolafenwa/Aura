//! Boundary and measurement tests for the interpreter's task-stack headroom
//! probe. The published writable limit must exclude the allocator's guard
//! layout, and the headroom reserve must exceed the stack the interpreter
//! consumes between two consecutive Aura calls while staying below the
//! first-call headroom of the opt-in minimum stack, under every build profile
//! the suite runs under.

use crate::mir_runtime::{MIR_RUNTIME_STACK_SIZE, TASK_STACK_HEADROOM_RESERVE};
use crate::runtime_value::{
    task_stack_bounds, task_stack_observation, ROOT_LIGHTWEIGHT_TASK_STACK_SIZE,
};
use crate::{lower_source_to_mir, run_mir};
use corosensei::stack::{DefaultStack, Stack};
use std::sync::Mutex;

/// Serializes the tests that use the process-global observation recorder.
static RECORDER: Mutex<()> = Mutex::new(());

/// A capacity no other test requests, and a multiple of every maintained
/// host page size, so the page-rounded writable mapping equals the request
/// exactly and observations can be attributed to the measured child.
const MEASURED_STACK_BYTES: usize = 15 * 1024 * 1024 + 64 * 1024;
const MEASURED_DEPTH: usize = 12;

/// The writable capacity of a stack of `MEASURED_STACK_BYTES`. It is the
/// request on Unix. Windows takes its guard pages and the thread stack
/// guarantee out of the request.
fn measured_capacity() -> usize {
    let stack = DefaultStack::new(MEASURED_STACK_BYTES).expect("the measured stack allocates");
    task_stack_bounds(&stack).usable_capacity()
}

/// Slack the reserve must keep above the worst measured transition: the
/// failing probe still formats its diagnostic on the same stack, and a
/// moderately larger interpreter frame must become a diagnostic, not a fault.
const TRANSITION_MARGIN: usize = 32 * 1024;
/// Slack a fresh minimum-capacity task must keep above the reserve at its
/// first call, so the 256 KiB opt-in minimum can always run one call.
const FIRST_CALL_MARGIN: usize = 16 * 1024;
/// Stack the entry thread keeps beyond the deepest legal recursion when a
/// program without tasks runs its entry off-coroutine, where no probe runs.
const ENTRY_THREAD_MARGIN: usize = 4 * 1024 * 1024;

struct Shape {
    name: &'static str,
    definitions: &'static str,
    start_arguments: &'static str,
}

const SHAPES: &[Shape] = &[
    Shape {
        name: "plain recursion",
        definitions: "def descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    return descend(depth - 1) + 1\n",
        start_arguments: "descend, 12",
    },
    Shape {
        name: "packed lambda per level",
        definitions: "type Step = Callable[def(depth: int64) -> int64]\n\ndef descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    step = Step(lambda depth: descend(depth))\n    return step(depth - 1) + 1\n",
        start_arguments: "descend, 12",
    },
    Shape {
        name: "list.map callback per level",
        definitions: "type Step = Callable[def(depth: int64) -> int64]\n\ndef descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    pending: list[int64] = [depth - 1]\n    step = Step(descend)\n    results = pending.map(step)\n    return results[0] + 1\n",
        start_arguments: "descend, 12",
    },
    Shape {
        name: "sort key callback per level",
        definitions: "type Keyed = Callable[def(depth: int64) -> int64]\n\ndef descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    mut pending: list[int64] = [depth - 1, depth - 1]\n    key = Keyed(descend)\n    pending.sort(key=key)\n    return pending[0] + 1\n",
        start_arguments: "descend, 12",
    },
    Shape {
        name: "generic trait dispatch per level",
        definitions: "trait Walker:\n    def step(self, depth: int64) -> int64\n\nclass Down:\n    marker: int64\n\nimpl Walker for Down:\n    def step(self, depth: int64) -> int64:\n        return descend(depth)\n\ndef walk[W: Walker](walker: W, depth: int64) -> int64:\n    return walker.step(depth)\n\ndef descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    return walk(Down(marker=depth), depth - 1) + 1\n",
        start_arguments: "descend, 12",
    },
    Shape {
        name: "method, match, and f-string per level",
        definitions: "enum Level:\n    Down(int64)\n    Bottom\n\nclass Counter:\n    total: int64\n\n    def descend_from(self, level: Level) -> int64:\n        match level:\n            case Level.Down(depth):\n                label = f\"level {depth} of {self.total}\"\n                if depth == 0:\n                    return label.len() - label.len()\n                return self.descend_from(Level.Down(depth - 1)) + 1\n            case Level.Bottom:\n                return 0\n\ndef descend(depth: int64) -> int64:\n    return Counter(total=depth).descend_from(Level.Down(depth))\n",
        start_arguments: "descend, 12",
    },
    Shape {
        name: "control.retry TaskCallable per level",
        definitions: "import control\n\ntype Worker = TaskCallable[def() -> Result[int64, str]]\n\ndef descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    owned = depth - 1\n    worker = Worker(lambda: Result.Ok(descend(owned) + 1))\n    match control.retry(worker, max_attempts=1):\n        case Result.Ok(value):\n            return value\n        case Result.Err(message):\n            return -1\n",
        start_arguments: "descend, 12",
    },
    Shape {
        name: "generic function per level",
        definitions: "def descend[T](depth: int64, marker: T) -> int64:\n    if depth == 0:\n        return 0\n    return descend[T](depth - 1, marker) + 1\n",
        start_arguments: "descend[str], 12, \"marker\"",
    },
    Shape {
        name: "resource close per level",
        definitions: r#"
class Step:
    depth: int64

    def close(mut self):
        if self.depth > 0:
            _ = descend(self.depth - 1)

def descend(depth: int64) -> int64:
    with step = Step(depth=depth):
        pass
    return depth
"#,
        start_arguments: "descend, 12",
    },
];

const TASK_RESULT_MATCH: &str = "        match task.result(timeout=60s):\n            case TaskResult.Ready(value):\n                print(f\"ready {value}\")\n            case TaskResult.Error(message):\n                print(f\"error {message}\")\n            case TaskResult.TimedOut:\n                print(\"timed out\")\n            case TaskResult.Cancelled:\n                print(\"cancelled\")\n";

fn child_program(shape: &Shape, stack: usize) -> String {
    format!(
        "{definitions}\ndef main():\n    with TaskGroup() as group:\n        task = group.start_with_stack({stack}, {start})\n{TASK_RESULT_MATCH}",
        definitions = shape.definitions,
        start = shape.start_arguments,
    )
}

/// The program entry runs as the root lightweight task only when the module
/// uses tasks; a program without them runs `main` on the entry thread, where
/// no coroutine bounds are published. Starting one trivial child makes the
/// root recursion execute on the root task's coroutine stack.
fn root_program(shape: &Shape, depth: usize) -> String {
    format!(
        "{definitions}\ndef idle() -> None:\n    return None\n\ndef main():\n    with TaskGroup() as group:\n        group.start_soon(idle)\n    print(descend({depth}))\n",
        definitions = shape.definitions,
    )
}

struct Measurement {
    shape: &'static str,
    /// Largest stack consumption between two consecutive probes.
    worst_transition: usize,
    /// Average consumption per probe across the whole descent.
    mean_transition: usize,
    /// Stack the task entry consumed before its first probe.
    entry_overhead: usize,
    probes: usize,
}

/// Runs every shape on a child whose stack the recorder can identify and
/// measures the interpreter's stack consumption between headroom probes. The
/// recorder lock must be held.
fn measure_shapes() -> Vec<Measurement> {
    SHAPES
        .iter()
        .map(|shape| {
            let source = child_program(shape, MEASURED_STACK_BYTES);
            let mir = lower_source_to_mir(&source)
                .unwrap_or_else(|error| panic!("{} lowers: {error:?}", shape.name));
            task_stack_observation::start_recording();
            let output = run_mir(&mir);
            let capacity = measured_capacity();
            let headrooms = task_stack_observation::stop_recording(capacity);
            let output = output.unwrap_or_else(|error| panic!("{} runs: {error:?}", shape.name));
            assert_eq!(
                output.stdout,
                format!("ready {MEASURED_DEPTH}\n"),
                "{} completes on a {MEASURED_STACK_BYTES}-byte child",
                shape.name
            );
            assert!(
                headrooms.len() > MEASURED_DEPTH,
                "{} probes every level: {} probes",
                shape.name,
                headrooms.len()
            );
            let worst_transition = headrooms
                .windows(2)
                .map(|pair| pair[0].saturating_sub(pair[1]))
                .max()
                .unwrap_or_default();
            assert!(
                worst_transition > 0,
                "{} consumes stack between probes",
                shape.name
            );
            let deepest = headrooms.iter().copied().min().unwrap_or_default();
            let mean_transition = headrooms[0].saturating_sub(deepest) / (headrooms.len() - 1);
            Measurement {
                shape: shape.name,
                worst_transition,
                mean_transition,
                entry_overhead: capacity.saturating_sub(headrooms[0]),
                probes: headrooms.len(),
            }
        })
        .collect()
}

fn worst_measurement(measurements: &[Measurement]) -> &Measurement {
    measurements
        .iter()
        .max_by_key(|measurement| measurement.worst_transition)
        .expect("at least one shape is measured")
}

#[test]
fn task_stack_bounds_exclude_the_allocator_guard_layout() {
    let requested = 256 * 1024;
    let stack = DefaultStack::new(requested).expect("a 256 KiB stack allocates");
    let bounds = task_stack_bounds(&stack);
    assert_eq!(bounds.base, stack.base().get());
    assert!(
        bounds.usable_limit > stack.limit().get(),
        "the writable limit lies above the mapping start that `Stack::limit` reports"
    );
    assert!(bounds.usable_limit < bounds.base);
    #[cfg(unix)]
    {
        assert_eq!(
            bounds.usable_limit - stack.limit().get(),
            crate::runtime_value::host_page_size(),
            "Unix stacks carry exactly one guard page below the writable capacity"
        );
        assert_eq!(
            bounds.usable_capacity(),
            requested,
            "a page-multiple request maps exactly that much writable stack"
        );
    }
    assert!(bounds.usable_capacity() >= corosensei::stack::MIN_STACK_SIZE);
}

#[test]
fn interpreter_call_transitions_fit_inside_the_headroom_reserve() {
    let _serialized = RECORDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let measurements = measure_shapes();
    for measurement in &measurements {
        eprintln!(
            "task stack transition: {} -> worst {} bytes, mean {} bytes, entry {} bytes, {} probes",
            measurement.shape,
            measurement.worst_transition,
            measurement.mean_transition,
            measurement.entry_overhead,
            measurement.probes
        );
    }
    let worst = worst_measurement(&measurements);
    assert!(
        worst.worst_transition + TRANSITION_MARGIN <= TASK_STACK_HEADROOM_RESERVE,
        "the {} bytes the interpreter consumed between consecutive calls for `{}` plus a \
         {TRANSITION_MARGIN} byte margin must fit the {TASK_STACK_HEADROOM_RESERVE} byte \
         headroom reserve, or a call can pass the probe and fault on the guard page",
        worst.worst_transition,
        worst.shape
    );

    let minimum_stack = usize::try_from(aura_core::task::MIN_TASK_STACK_BYTES).expect("positive");
    let entry_overhead = measurements
        .iter()
        .map(|measurement| measurement.entry_overhead)
        .max()
        .unwrap_or_default();
    assert!(
        TASK_STACK_HEADROOM_RESERVE + entry_overhead + FIRST_CALL_MARGIN <= minimum_stack,
        "a fresh {minimum_stack} byte minimum task spends {entry_overhead} bytes before its \
         first probe and must still hold the {TASK_STACK_HEADROOM_RESERVE} byte reserve plus \
         a {FIRST_CALL_MARGIN} byte margin, or the opt-in minimum could never run one call"
    );
}

#[test]
fn root_task_recursion_is_a_diagnostic_before_the_stack_can_fault() {
    let _serialized = RECORDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Nothing but the stack bounds the depth of calls, so recursion that
    // never ends must stop at the headroom probe on the root task's stack.
    let shape = &SHAPES[1];
    let mir =
        lower_source_to_mir(&root_program(shape, 100_000_000)).expect("root recursion lowers");
    let error = run_mir(&mir).expect_err("root recursion past the stack fails");
    assert_eq!(error.code, "AU4005");
    assert!(
        error.message.contains("call stack exhausted while calling"),
        "{error}"
    );
}

#[test]
fn exhausting_task_stacks_closes_each_resource_and_manager_in_order() {
    let source = r#"
class Inner:
    depth: int64

    def close(mut self):
        print(f"inner {self.depth}")

class Resource:
    depth: int64

    def close(mut self):
        with inner = Inner(depth=self.depth):
            print(f"close {self.depth}")

class Manager:
    depth: int64

    def __exit__(mut self):
        print(f"exit {self.depth}")

def descend(depth: int64) -> int64:
    with outer = Resource(depth=depth), inner = Manager(depth=depth):
        print(depth)
        return descend(depth + 1)

def main():
    for size in range(655360, 786433, 4096):
        with group = TaskGroup():
            job = group.start_with_stack(size, descend, 0)
            match job.result():
                case TaskResult.Error(message):
                    print(message)
                case _:
                    print("unexpected outcome")
"#;
    let mir = lower_source_to_mir(source).expect("recursive scopes lower");
    let output = run_mir(&mir).expect("the parent observes each stack exhaustion");
    let mut lines = output.stdout.lines().peekable();
    for size in (655360..=786432).step_by(4096) {
        let mut entered = 0;
        while lines
            .peek()
            .is_some_and(|line| line.parse::<usize>().is_ok())
        {
            assert_eq!(lines.next().unwrap(), entered.to_string(), "stack {size}");
            entered += 1;
        }
        assert!(entered > 0, "stack {size} should enter at least one scope");
        for depth in (0..entered).rev() {
            assert_eq!(
                lines.next().unwrap(),
                format!("exit {depth}"),
                "stack {size}"
            );
            assert_eq!(
                lines.next().unwrap(),
                format!("close {depth}"),
                "stack {size}"
            );
            assert_eq!(
                lines.next().unwrap(),
                format!("inner {depth}"),
                "stack {size}"
            );
        }
        assert_eq!(
            lines.next(),
            Some("call stack exhausted while calling `descend`"),
            "stack {size}"
        );
    }
    assert!(lines.next().is_none());
}

#[test]
fn the_entry_stack_is_the_root_task_stack() {
    // The interpreter gives the program's first task the whole stack its
    // thread was started with, so deep recursion has the same room with
    // and without tasks.
    const {
        assert!(ROOT_LIGHTWEIGHT_TASK_STACK_SIZE == 64 * 1024 * 1024);
        assert!(MIR_RUNTIME_STACK_SIZE >= 4 * ROOT_LIGHTWEIGHT_TASK_STACK_SIZE);
        assert!(TASK_STACK_HEADROOM_RESERVE + ENTRY_THREAD_MARGIN < MIR_RUNTIME_STACK_SIZE);
    }
}
