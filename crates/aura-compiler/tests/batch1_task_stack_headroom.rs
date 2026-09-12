//! A lightweight task whose coroutine stack would be exhausted by the next
//! Aura call reports a diagnostic naming the callee instead of faulting on
//! the stack's guard page, on any build profile.

use aura_compiler::{lower_source_to_mir, run_mir};

const DEEP_SOURCE: &str = "def descend(depth: int64) -> int64:\n    if depth == 0:\n        return 0\n    return descend(depth - 1) + 1\n\ndef main():\n    with TaskGroup() as group:\n        task = group.start_with_stack(262144, descend, 200)\n        match task.result(timeout=5s):\n            case TaskResult.Ready(value):\n                print(f\"ready {value}\")\n            case TaskResult.Error(message):\n                print(f\"error {message}\")\n            case TaskResult.TimedOut:\n                print(\"timed out\")\n            case TaskResult.Cancelled:\n                print(\"cancelled\")\n";

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
