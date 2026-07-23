## Goal

Validate and fix the latest review findings covering the remaining native bare-`None` coercion holes, wait-site inconsistencies, checker false positives, and the `fs.write_bytes(path, [])` run-vs-build divergence.

## Work Completed

- Added failing-first regressions in [crates/aura/tests/cli.rs](/Users/johnolafenwa/source2/Aurora/crates/aura/tests/cli.rs) and [crates/aurora-compiler/src/sema_tests.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/sema_tests.rs) for the latest review surface: native bare-`None` coercions through collections and nested option fields, no-timeout `Queue`/`Task` wait helpers, cancellation during `sleep(...)`, immediate `wait_any([])`, `fs.write_bytes(path, [])`, and move-type `Vec[...]` literal checking.
- Fixed native coercion gaps across [crates/aurora-compiler/src/native_codegen.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/native_codegen.rs) and [crates/aurora-compiler/src/mir.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/mir.rs) by retargeting temporaries and coercing collection literals, collection member calls, and class-constructor field values against their declared element or field types, so bare `None` now round-trips correctly through `Vec`, `Map`, `Set`, `Queue`, and nested `Option[...]` class fields in direct-built binaries.
- Fixed the reviewed wait-site inconsistencies in [crates/aurora-compiler/src/mir_runtime.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/mir_runtime.rs) and [crates/aurora-compiler/src/native_runtime.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/native_runtime.rs): no-timeout `Queue.get_or_none()` / `Queue.get_or(default)` and `Task.result_or_none()` / `Task.result_or(default)` are now immediate non-blocking checks, `wait_any([])` returns `WaitAny.TimedOut` immediately, `sleep(...)` wakes on cancellation without killing the task so code can observe `cancelled()`, and empty byte vectors are accepted by `fs.write_bytes(...)` in both runtimes.
- Removed the checker false positive for move-type collection literals in [crates/aurora-compiler/src/sema.rs](/Users/johnolafenwa/source2/Aurora/crates/aurora-compiler/src/sema.rs) by avoiding early consumption during hint-based list/map/set typing. The other two externally reported claims from this review, queue iteration parking forever under cancellation and the `match get_tag(b)` false positive, did not reproduce on the current tree and therefore did not require production-code changes in this pass.
- Updated the maintained concurrency surface in [examples/concurrency/bounded_queue.au](/Users/johnolafenwa/source2/Aurora/examples/concurrency/bounded_queue.au), [examples/concurrency/task_group_start.au](/Users/johnolafenwa/source2/Aurora/examples/concurrency/task_group_start.au), [examples/concurrency/task_group_start_soon.au](/Users/johnolafenwa/source2/Aurora/examples/concurrency/task_group_start_soon.au), [examples/concurrency/task_group_associated_method.au](/Users/johnolafenwa/source2/Aurora/examples/concurrency/task_group_associated_method.au), [tutorials/13-concurrency.md](/Users/johnolafenwa/source2/Aurora/tutorials/13-concurrency.md), [tutorials/14-current-language-surface.md](/Users/johnolafenwa/source2/Aurora/tutorials/14-current-language-surface.md), and [examples/README.md](/Users/johnolafenwa/source2/Aurora/examples/README.md) so blocking examples now use explicit timeouts and the documented semantics match the implemented no-timeout convenience helpers.
- Updated the queue fairness CLI regression in [crates/aura/tests/cli.rs](/Users/johnolafenwa/source2/Aurora/crates/aura/tests/cli.rs) to use explicit task-result timeouts, aligning the broad end-to-end suite with the maintained non-blocking `Task.result_or(...)` contract.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`
- Targeted manual confirmations during the pass:
  - `./target/debug/aura run /tmp/aurora_cluster_a_debug.au`
  - `./target/debug/aura build --backend direct -o /tmp/aurora_cluster_a_debug.bin /tmp/aurora_cluster_a_debug.au && /tmp/aurora_cluster_a_debug.bin`
  - `./target/debug/aura run /tmp/aurora_sleep_cancel_smoke.au`
  - `./target/debug/aura build --backend direct -o /tmp/aurora_sleep_cancel_smoke.bin /tmp/aurora_sleep_cancel_smoke.au && /tmp/aurora_sleep_cancel_smoke.bin`
  - `./target/debug/aura run /tmp/aurora_fs_write_empty_smoke.au`
  - `./target/debug/aura build --backend direct -o /tmp/aurora_fs_write_empty_smoke.bin /tmp/aurora_fs_write_empty_smoke.au && /tmp/aurora_fs_write_empty_smoke.bin`

## Follow-up

- No additional follow-up is required for this pass beyond the existing broader performance and memory items already tracked on the task board. The two externally reported claims that did not reproduce should only be reopened if a stable local repro appears.
