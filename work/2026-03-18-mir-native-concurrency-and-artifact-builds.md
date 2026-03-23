# 2026-03-18 MIR-Native Concurrency And Artifact Builds

## Goal

Finish the current backend surface by making the implemented concurrency features run natively through MIR and by changing `aura build` to package lowered MIR instead of raw source.

## Work Completed

- added failing MIR-native tests for the current concurrency examples:
  - `channels_spawn`
  - `send_result`
  - `spawn_detached`
  - `select_timeout`
  - `select_send`
  - `task_group_select`
  - `task_group_cancel`
- extended MIR to preserve concurrency semantics explicitly:
  - `Rvalue::Spawn`
  - structured `MirSelectKind` arms for `recv`, `send`, and `after`
  - cleanup instructions that carry cancellation intent for non-local exits
- extended the MIR runtime to support:
  - `channel()`
  - `task_group()`
  - `cancelled()`
  - `after(...)`
  - channel methods: `clone`, `send`, `recv`, `close`
  - task methods: `clone`, `join`
  - task-group cancellation and managed cleanup
  - `spawn` and `spawn detached`
  - `select` over receive, send, and timer arms
- removed the backend fallback gate for the current implemented surface so `run_source_via_mir(...)` now always lowers and runs MIR directly
- changed `aura build` to lower once at build time, serialize MIR, and generate a launcher that deserializes the MIR artifact and calls `run_mir(...)`
- updated the current-facing docs and task board to reflect that the current implemented Aurora surface now runs natively through MIR

## Verification

- `cargo test`
- `cargo run -p aura -- run-mir examples/resources/with_resource.au`
- `cargo run -p aura -- build -o ./target/aurora-point-mir examples/point.au`
- `./target/aurora-point-mir`
- `cargo run -p aura -- build -o ./target/aurora-channels-mir examples/concurrency/channels_spawn.au`
- `./target/aurora-channels-mir`

## Follow-up

- replace the generated Rust launcher approach with real MIR-native code generation
- raise backend and compiler coverage gates from measured to enforced
- continue runtime work for proposal items that are not yet part of the implemented bootstrap surface
