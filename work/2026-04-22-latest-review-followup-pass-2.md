## Goal

Validate and fix the newest review findings covering the remaining match-expression move-tracking false positive, queue-iteration cancellation hang, and the swap out-of-bounds message parity gap.

## Session

- Start: 2026-04-22 14:33:27 BST
- Stop: 2026-04-22 15:38:34 BST
- Elapsed: 01:05:07
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Added failing-first coverage for the remaining Claude review issues:
  - `run_and_direct_backend_allow_match_expression_value_scrutinee_first_use`
  - `queue_iteration_exits_when_task_group_is_cancelled`
  - tightened `vec_swap_out_of_bounds_is_a_runtime_error`
  - `check-pass/match_expression_value_scrutinee_first_use.au`
- Fixed the match-expression scrutinee false positive in the checker by avoiding move-state-sensitive re-typechecking for `ExprKind::Match` scrutinee consumption.
- Removed the abandoned scope-wide task-group cancellation push/pop path from direct codegen and kept cancellation propagation targeted to queue iteration only.
- Added an internal queue receive helper for `for value in queue:` when a `Queue[T]` loop runs inside an active `with TaskGroup()` scope:
  - MIR lowering now threads the nearest active `TaskGroup` into queue iteration
  - MIR runtime merges the current cancellation context with that task-group child cancellation for the internal receive
  - direct runtime/codegen use the same merged-cancellation receive path
- Aligned the direct-runtime vector swap out-of-bounds message with MIR so both backends report both indices.
- Narrowed the maintained tutorial wording in:
  - `tutorials/11-resource-management.md`
  - `tutorials/13-concurrency.md`
  - `tutorials/14-current-language-surface.md`
  so they document queue-iteration cancellation in the same `with TaskGroup()` scope rather than implying all waits in that scope observe `group.cancel()`.

## Verification

- `cargo fmt --all`
- `cargo test -p aura --test cli run_and_direct_backend_allow_match_expression_value_scrutinee_first_use -- --nocapture`
- `cargo test -p aura --test cli queue_iteration_exits_when_task_group_is_cancelled -- --nocapture`
- `cargo test -p aura --test cli cancelled_sleeping_children_resume_and_can_observe_cancellation -- --nocapture`
- `cargo test -p aura --test cli cancelled_yields_for_cpu_bound_lightweight_tasks -- --nocapture`
- `cargo test -p aura --test cli vec_swap_out_of_bounds_is_a_runtime_error -- --nocapture`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`

## Follow-up

- The current tree still emits a small set of pre-existing `dead_code` warnings in `runtime_value.rs` during Rust builds. They are unrelated to this pass.
