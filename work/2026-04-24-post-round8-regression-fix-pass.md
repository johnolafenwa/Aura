# April 24 Post-Round-8 Regression Fix Pass

## Goal

Fix the confirmed post-Round-8 regressions in queue iteration and native direct-backend cleanup unwinding, keeping focused regression coverage and verification aligned.

## Work Completed

- Added failing-first CLI regression coverage for direct-backend `with` cleanup using the current mutated resource state when a called function traps.
- Added failing-first CLI regression coverage for `for value in queue:` waiting for a producer started through a standalone `TaskGroup()` value outside a `with TaskGroup` scope.
- Replaced the hidden empty task-group fallback in queue `for` lowering with an internal queue receive helper that waits on tasks registered as producers for that queue.
- Registered spawned tasks as queue producers for any `Queue` values reachable through their start arguments, including queues nested inside vectors, sets, maps, instances, and enum payloads.
- Implemented the registered-producer queue receive path in the MIR runtime and native direct runtime/codegen.
- Refreshed native direct-backend cleanup snapshots after mutations to active cleanup places, using a single runtime helper to avoid invalid Cranelift control flow and to release stale cleanup arguments correctly.

## Verification

- `cargo fmt --all --check`
- `CARGO_TARGET_DIR=/tmp/aurora-post8-target cargo check -p aurora-compiler`
- `CARGO_TARGET_DIR=/tmp/aurora-post8-target cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-post8-target cargo test -p aurora-compiler --lib -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-post8-target cargo test -p aura direct_backend_callee_trap_cleanup_uses_current_resource_state --test cli -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-post8-target cargo test -p aura --test cli queue_iteration_ -- --test-threads=1 --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-post8-target cargo test -p aura --test cli direct_backend_unwinds_with_resources -- --test-threads=1 --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-post8-target cargo test -p aura --test cli -- --test-threads=1 --nocapture`

## Follow-up

- No new regression was found in the exercised queue iteration and native cleanup areas. Existing dead-code warnings in `aurora-compiler` remain unchanged by this pass.
