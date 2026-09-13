//! A lightweight task whose coroutine stack would be exhausted by the next
//! Aura call reports a diagnostic naming the callee instead of faulting on
//! the stack's guard page, on any build profile and at every boundary: the
//! opt-in minimum child stack, a larger child whose probe alignment used to
//! fault, and the root task that executes the program entry. A guard-page
//! fault kills the test process, so each sweep is its own acceptance test.
//!
//! The direct backend has only the 256-call depth limit and no headroom
//! probe, so these tests exercise the MIR interpreter alone.

use aura_compiler::{lower_source_to_mir, run_mir};

const DEEP_SOURCE: &str = "def descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    return descend(depth - 1) + 1\n\ndef main():\n    with TaskGroup() as group:\n        task = group.start_with_stack(262144, descend, 200)\n        match task.result(timeout=5s):\n            case TaskResult.Ready(value):\n                print(f\"ready {value}\")\n            case TaskResult.Error(message):\n                print(f\"error {message}\")\n            case TaskResult.TimedOut:\n                print(\"timed out\")\n            case TaskResult.Cancelled:\n                print(\"cancelled\")\n";

/// One packed-callable call plus one ordinary call per level: the shape
/// whose interpreter frames are the largest of the measured call shapes.
const PACKED_DEFINITIONS: &str = "type Step = Callable[def(depth: int64) -> int64]\n\ndef descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    step = Step(lambda depth: descend(depth))\n    return step(depth - 1) + 1\n";

const EXHAUSTED: &str = "task stack exhausted while calling";
const MINIMUM_STACK: usize = 262_144;
/// The child capacity on which packed recursion to depth 12 crossed the
/// guard page before the reserve was sized from measured transitions.
const LARGER_STACK: usize = 3 * 1024 * 1024 + 64 * 1024;

/// Starts one child per depth from 0 through `max_depth` on a `stack`-byte
/// coroutine stack and prints each outcome on its own line.
fn child_sweep_source(stack: usize, max_depth: usize) -> String {
    format!(
        "{PACKED_DEFINITIONS}\ndef main():\n    with TaskGroup() as group:\n        for depth in range(stop={stop}):\n            task = group.start_with_stack({stack}, descend, depth)\n            match task.result(timeout=60s):\n                case TaskResult.Ready(value):\n                    print(f\"depth {{depth}} ready {{value}}\")\n                case TaskResult.Error(message):\n                    print(f\"depth {{depth}} error {{message}}\")\n                case TaskResult.TimedOut:\n                    print(f\"depth {{depth}} timed out\")\n                case TaskResult.Cancelled:\n                    print(f\"depth {{depth}} cancelled\")\n",
        stop = max_depth + 1,
    )
}

enum Outcome {
    Ready,
    Exhausted,
}

fn parse_sweep(stdout: &str, stack: usize) -> Vec<Outcome> {
    stdout
        .lines()
        .enumerate()
        .map(|(depth, line)| {
            let expected_prefix = format!("depth {depth} ");
            let rest = line
                .strip_prefix(&expected_prefix)
                .unwrap_or_else(|| panic!("line {depth} reports its depth: {line}"));
            if let Some(value) = rest.strip_prefix("ready ") {
                assert_eq!(value, depth.to_string(), "depth {depth} returns its depth");
                return Outcome::Ready;
            }
            let message = rest
                .strip_prefix("error ")
                .unwrap_or_else(|| panic!("depth {depth} is ready or an error: {line}"));
            assert!(
                message.starts_with(EXHAUSTED),
                "depth {depth} fails with the stack diagnostic, not another error: {message}"
            );
            assert!(
                message.contains("`descend`") || message.contains("`descend::__lambda_"),
                "depth {depth} names the callee: {message}"
            );
            let remaining = message
                .split(": ")
                .nth(1)
                .and_then(|tail| tail.split(' ').next())
                .and_then(|digits| digits.parse::<usize>().ok())
                .unwrap_or_else(|| panic!("depth {depth} reports the remaining bytes: {message}"));
            assert!(
                remaining < stack,
                "depth {depth} reports writable headroom below the requested {stack} bytes: {message}"
            );
            Outcome::Exhausted
        })
        .collect()
}

fn run_sweep(stack: usize, max_depth: usize) -> Vec<Outcome> {
    let mir = lower_source_to_mir(&child_sweep_source(stack, max_depth)).expect("sweep lowers");
    let output = run_mir(&mir).expect("every child failure is observed by the parent");
    let outcomes = parse_sweep(&output.stdout, stack);
    assert_eq!(outcomes.len(), max_depth + 1, "{}", output.stdout);
    let first_failure = outcomes
        .iter()
        .position(|outcome| matches!(outcome, Outcome::Exhausted))
        .unwrap_or(outcomes.len());
    assert!(
        outcomes[first_failure..]
            .iter()
            .all(|outcome| matches!(outcome, Outcome::Exhausted)),
        "once a depth exhausts the stack every deeper start does too:\n{}",
        output.stdout
    );
    assert!(
        matches!(outcomes[0], Outcome::Ready),
        "a fresh {stack} byte task runs at least its entry call:\n{}",
        output.stdout
    );
    outcomes
}

#[test]
fn exhausting_a_minimum_task_stack_reports_a_diagnostic() {
    let mir = lower_source_to_mir(DEEP_SOURCE).expect("deep recursion lowers");
    let output = run_mir(&mir).expect("the parent observes the child's failure and completes");
    assert!(
        output.stdout.contains("error")
            && output
                .stdout
                .contains("task stack exhausted while calling `descend`"),
        "{}",
        output.stdout
    );
}

#[test]
fn minimum_child_stack_boundary_is_a_diagnostic_at_every_depth() {
    let outcomes = run_sweep(MINIMUM_STACK, 48);
    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, Outcome::Exhausted)),
        "48 packed levels exceed a {MINIMUM_STACK} byte stack on every profile"
    );
}

#[test]
fn larger_child_stack_boundary_never_crosses_the_guard_page() {
    // Depth 12 alone faulted with SIGBUS on this capacity when the reserve
    // was below one call transition; the sweep crosses every alignment.
    run_sweep(LARGER_STACK, 24);
}

#[test]
fn root_task_recursion_reports_a_diagnostic_instead_of_faulting() {
    // Starting one child makes the entry run as the root lightweight task on
    // its own coroutine stack, where the headroom probe is active.
    let source = format!(
        "{PACKED_DEFINITIONS}\ndef idle() -> None:\n    return None\n\ndef main():\n    with TaskGroup() as group:\n        group.start_soon(idle)\n    print(descend(1000))\n"
    );
    let mir = lower_source_to_mir(&source).expect("root recursion lowers");
    let error = run_mir(&mir).expect_err("root recursion past both limits fails");
    let message = error.to_string();
    assert!(
        message.contains(EXHAUSTED) || message.contains("maximum call depth of 256 exceeded"),
        "the root task ends with the stack or depth diagnostic: {message}"
    );
}
