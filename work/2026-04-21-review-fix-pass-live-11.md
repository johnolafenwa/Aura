## Goal

Fix the 11 still-live defects confirmed from `/tmp/aurora_review`, covering `Option`-match inference, `aura run` stdout flushing on runtime errors, `TaskGroup` scope semantics, cooperative cancellation, task failure surfacing, literal-`match` and `with` move tracking, self-receiver false positives, and `Vec.insert` / `Vec.swap` out-of-bounds handling.

## Session

- Start: 2026-04-21 22:34:16 BST
- Stop: 2026-04-22 00:27:33 BST
- Elapsed: 01:53:17
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Added or updated failing-first regressions for the still-live defects across CLI, checker fixtures, and runtime/native-runtime coverage.
- Fixed `aura run` so buffered stdout is preserved when runtime diagnostics abort execution, and threaded that partial output through the CLI diagnostic path.
- Restored unannotated `Queue.get_or_none()` / `Task.result_or_none()` match inference so `Some` / `None` arms execute correctly in both `run` and direct-built binaries.
- Reworked `TaskGroup` cleanup so scope exit still joins short-lived child work but cancels children that are parked forever in cancellation-aware waits before joining them, keeping blocked scope shutdown from hanging.
- Fixed cooperative cancellation for CPU-bound lightweight tasks in the direct runtime by removing leaked child cancellation scope state.
- Surfaced task failures as `TaskResult.Error(...)`, `WaitAny.Error(...)`, and `WaitAll.Error(...)` instead of aborting the Aurora program, and updated the direct-runtime task boundary/panic plumbing to preserve those results safely.
- Restored move-state propagation after literal `match` blocks and `with` blocks so post-block use-after-move is rejected consistently again.
- Fixed the self-receiver method false positive when binding a value-returning receiver call to a name.
- Changed `Vec.insert(...)` and `Vec.swap(...)` out-of-bounds behavior from silent discard/no-op to explicit runtime errors in both runtimes, and updated the maintained collection example/tutorial surface accordingly.
- Updated the maintained resource-management and concurrency tutorials to describe the corrected `TaskGroup` scope-exit behavior, and repaired the broad compiler test harnesses so the large scratch-corpus tests run on large-stack helper threads instead of overflowing the default test thread.
- Updated the native-runtime helper regression that previously expected `task-join-error` to abort; it now validates the surfaced `TaskResult.Error("boom")` contract directly.

## Verification

- `cargo fmt --all`
- `cargo test -p aura --test cli task_group_scope_exit_cancels_blocked_children -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aurora-compiler --lib broad_scratch_corpus_runtime_paths_do_not_panic -- --nocapture`
- `cargo test -p aurora-compiler --lib broad_scratch_corpus_checks_analysis_and_mir_lowering_do_not_panic -- --nocapture`
- `cargo test -p aurora-compiler --lib native_runtime::tests::direct_runtime_helper_errors_surface_expected_diagnostics -- --nocapture`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`

## Follow-up

- No additional blockers remain from the eleven confirmed `/tmp/aurora_review` defects. The broader coverage and example-sync work on the task board remains ongoing repo maintenance, not part of this completed pass.
