## Goal

Validate and fix the latest follow-up review findings after the live eleven-defect pass, covering indirect bare-`None` matching, `match`-expression move tracking, unobserved task panic surfacing, remaining vector OOB holes, early-return ownership diagnostics, and the stale maintained concurrency example.

## Work Completed

- Added failing-first regressions for the new follow-up defects in [crates/aura/tests/cli.rs](/Users/johnolafenwa/source2/Aurora/crates/aura/tests/cli.rs) and compiler fixtures under [crates/aurora-compiler/tests/fixtures/check-fail/match_expression_move_persists.au](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/tests/fixtures/check-fail/match_expression_move_persists.au) and [crates/aurora-compiler/tests/fixtures/check-pass/return_only_branches_do_not_leak_moves.au](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/tests/fixtures/check-pass/return_only_branches_do_not_leak_moves.au).
- Fixed direct-backend opaque class construction in [crates/aurora-compiler/src/native_codegen.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/native_codegen.rs) so indirect `T?` fields initialized with bare `None` now coerce through the declared field type and match correctly in native binaries.
- Fixed checker control-flow ownership in [crates/aurora-compiler/src/sema.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/sema.rs): only fallthrough `if` / statement-`match` branches now merge move state, and `match` expressions now propagate consumed move-type arm values through both typing and consume tracking.
- Fixed unread task failure surfacing across the MIR and direct runtimes in [crates/aurora-compiler/src/runtime_value.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/runtime_value.rs), [crates/aurora-compiler/src/mir_runtime.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/mir_runtime.rs), and [crates/aurora-compiler/src/native_runtime.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/native_runtime.rs) by tracking observed task failures and making `TaskGroup()` scope exit report unread child errors instead of silently swallowing them.
- Aligned `Vec.set(...)` and `Vec.remove(...)` with the newer `insert` / `swap` behavior so out-of-bounds indices now raise runtime errors in both runtimes, while in-bounds calls still return `Option.Some(...)`.
- Repaired the maintained concurrency example [examples/concurrency/task_group_wait_helpers.au](/Users/johnolafenwa/source2/Aurora/examples/concurrency/task_group_wait_helpers.au) to use a mutable task list, and refreshed the concurrency/current-surface tutorials to document the updated task-group and vector OOB behavior.

## Verification

- `cargo fmt --all`
- `cargo test -p aura --test cli -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `./target/debug/aura check examples/concurrency/task_group_wait_helpers.au`
- Targeted manual confirmations during the pass:
  - `./target/debug/aura run /tmp/aurora_option_indirect_none.au`
  - `./target/debug/aura build --backend direct -o /tmp/aurora_option_indirect_none.bin /tmp/aurora_option_indirect_none.au && /tmp/aurora_option_indirect_none.bin`
  - `./target/debug/aura run /tmp/aurora_task_unobserved_panic.au`
  - `./target/debug/aura run /tmp/aurora_vec_set_remove_oob.au`

## Follow-up

- No new follow-up work is required for this pass beyond the existing broader coverage and performance tracks already recorded on the task board.
