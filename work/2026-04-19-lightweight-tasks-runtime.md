# 2026-04-19 Lightweight Tasks Runtime

- Session start: 2026-04-19 11:19:35 BST

## Goal

- Replace thread-backed Aurora tasks with scheduler-backed lightweight tasks while preserving the maintained queue/task/select surface.

## Work Completed

- Replaced MIR `spawn` / `TaskGroup.start(...)` task creation so Aurora tasks now run on the coroutine scheduler in `runtime_value.rs` instead of `thread::spawn(...)`.
- Routed the direct native runtime onto the same scheduler-backed task model by replacing `aurora_direct_spawn_call(...)` OS-thread spawning and changing the direct `main` wrapper to execute through `aurora_direct_run_root(...)`.
- Added scheduler task-local cancellation propagation so direct-runtime helpers such as `cancelled()`, `sleep(...)`, queue waits, and socket waits still observe the correct task cancellation scope after many Aurora tasks share one host thread.
- Added a maintained scalability regression that verifies thousands of waiting Aurora tasks do not scale the process thread count with task count, and adapted the direct-runtime unit coverage to execute spawn/join behavior under an active scheduler.
- Kept the maintained runtime recursion-depth diagnostic working by sizing coroutine task stacks to preserve the existing 256-call MIR safety limit.
- Preserved structured runtime diagnostics through the lightweight-task scheduler so MIR runtime errors such as division-by-zero still retain their source spans instead of being flattened to plain strings at the scheduler boundary.
- Fixed the CLI direct-build runtime-artifact lookup to use Cargo’s emitted staticlib artifact path before falling back to the filesystem scan, so repeated local compiler builds no longer make direct native builds fail with ambiguous hashed `libaurora_compiler-*.a` archives.

## Verification

- `cargo test -p aurora-compiler tests::lightweight_tasks_scale_to_thousands_of_waiting_tasks -- --nocapture`
- `cargo test -p aurora-compiler tests::mir_runtime_reports_recursion_limit_before_overflowing_the_host_stack -- --nocapture`
- `cargo test -p aurora-compiler native_codegen::tests::direct_backend_emits_object_for_module_examples -- --nocapture`
- `cargo test -p aurora-compiler native_runtime::tests -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures run_fail_fixtures_match_expected_diagnostics -- --nocapture`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- Async file I/O is still separate work; this pass only replaces the task runtime, not the synchronous file APIs.
