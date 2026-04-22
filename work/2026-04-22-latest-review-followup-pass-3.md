## Goal

Validate and fix the latest review finding covering queue iteration hanging when a sibling task panics without closing the queue.

## Session

- Start: 2026-04-22 21:32:55 BST
- Stop: 2026-04-22 23:20:56 BST
- Elapsed: 01:48:01
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Added failing-first coverage for the new sibling-task-panic queue-iteration hang in [crates/aura/tests/cli.rs](/Users/johnolafenwa/source2/Aurora/crates/aura/tests/cli.rs) and [crates/aurora-compiler/src/runtime_value_tests.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/runtime_value_tests.rs).
- Extended task-group state in [crates/aurora-compiler/src/runtime_value.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/runtime_value.rs) so tasks register group-failure wake flags and queue iteration can observe unobserved sibling-task failure, not just explicit cancellation.
- Routed `Queue` iteration receives in both [crates/aurora-compiler/src/mir_runtime.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/mir_runtime.rs) and [crates/aurora-compiler/src/native_runtime.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/native_runtime.rs) through the shared task-group-aware receive helper.
- Fixed a missed-wake race in the lightweight scheduler so a wait that becomes ready between registration and queue insertion is put back onto the ready queue immediately instead of parking indefinitely.
- Added a short cleanup-probe settle path for task-group shutdown, so freshly spawned children have a chance to register their blocking waits before scope-exit cleanup decides whether to cancel or keep waiting.
- Corrected direct-runtime `Queue.get_or*` and `Task.result_or*` fallback handling so defaults are cloned rather than consumed, and aligned MIR/native no-timeout fallback probes with the documented immediate non-blocking behavior.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aurora-compiler queue_iteration_wait_wakes_for_unobserved_task_group_failure -- --nocapture`
- Manual `aura run` repro for the sibling-panic queue-iteration case now exits immediately with the original bounds error instead of hanging.
- Manual `aura build --backend direct` plus direct binary run for the same repro now exits immediately with the same bounds error; the clean direct build took about 108 seconds because it paid the native-static-libs/direct-build warmup path.
- The broader `cargo test -p aurora-compiler --lib -- --nocapture` and `cargo test -p aurora-compiler --test fixtures -- --nocapture` sweeps had already passed earlier in the same code state before the final timeout-only CLI harness edits; this pass reran the fixtures suite and the focused runtime regression after the runtime changes.

## Follow-up

- Claude's out-of-scope long-term items remain unchanged: the known perf/memory costs, the 64 MiB file-read cap, the 1 MiB HTTP body cap, lack of multiline continuation, float divide-by-zero inconsistency, recursion-limit message off-by-one, and minor diagnostic-polish issues.
